//! Reusable PIO-based NEC IR decoder
//!
//! Uses RP2040's PIO to measure pulse/space durations in hardware.
//! PIO handles all timing-critical work, pushes durations to FIFO.
//! Software decodes the NEC protocol from the duration stream.
//!
//! # Usage
//! ```ignore
//! let mut decoder = NecIrDecoder::new(ir_pin, pio, sm);
//! loop {
//!     if let Some(cmd) = decoder.poll() {
//!         // Handle command
//!     }
//! }
//! ```
//!
//! # Polling Requirements
//! Call `poll()` frequently (every ~10-50ms) to avoid FIFO overflow.
//! The FIFO is 4 entries deep; a full NEC transmission produces ~34 values.
//! Missing values will cause decode failures but won't corrupt state.

use pio::{Assembler, JmpCondition, MovDestination, MovOperation, MovSource};
use rp2040_hal::pio::{PIOBuilder, PinDir, PioIRQ, Rx, SM0};

use crate::board::{IrReceiverPin, IrReceiverPinPio, Pio1, Pio1SM0};

/// PIO clock divisor: 125 gives 1MHz (1µs per PIO cycle)
/// Each measurement loop is 2 cycles, so resolution is 2µs
const PIO_CLOCK_DIVISOR: u16 = 125;

/// Timing thresholds in PIO cycles (at 2µs per count)
mod timing {
    // NEC leader pulse: 9ms = 4500 counts
    pub const LEADER_PULSE_MIN: u32 = 4000;
    pub const LEADER_PULSE_MAX: u32 = 5000;
    // NEC leader space: 4.5ms = 2250 counts
    pub const LEADER_SPACE_MIN: u32 = 2000;
    pub const LEADER_SPACE_MAX: u32 = 2500;
    // Bit pulse: 562.5µs = 281 counts
    pub const BIT_PULSE_MIN: u32 = 200;
    pub const BIT_PULSE_MAX: u32 = 400;
    // Bit 0 space: 562.5µs = 281 counts
    pub const BIT_ZERO_SPACE_MAX: u32 = 400;
    // Bit 1 space: 1687.5µs = 844 counts
    pub const BIT_ONE_SPACE_MIN: u32 = 600;
    pub const BIT_ONE_SPACE_MAX: u32 = 1000;
}

/// Decoded NEC command with address and command bytes
#[derive(Debug, Clone, Copy)]
pub struct IrCommand {
    pub address: u8,
    pub command: u8,
}

/// Known button codes for PicoBricks remote
#[allow(dead_code)]
pub mod buttons {
    pub const KEY_0: u8 = 0x16;
    pub const KEY_1: u8 = 0x0C;
    pub const KEY_2: u8 = 0x18;
    pub const KEY_3: u8 = 0x5E;
    pub const KEY_4: u8 = 0x08;
    pub const KEY_5: u8 = 0x1C;
    pub const KEY_6: u8 = 0x5A;
    pub const KEY_7: u8 = 0x42;
    pub const KEY_8: u8 = 0x52;
    pub const KEY_9: u8 = 0x4A;
    pub const KEY_STAR: u8 = 0x40;
    pub const KEY_HASH: u8 = 0x43;
    pub const KEY_UP: u8 = 0x46;
    pub const KEY_DOWN: u8 = 0x15;
    pub const KEY_LEFT: u8 = 0x44;
    pub const KEY_RIGHT: u8 = 0x07;
    pub const KEY_OK: u8 = 0x47;
}

/// Build the PIO program for IR pulse/space measurement
///
/// Output: alternating pulse (low) / space (high) durations in PIO cycles
/// Higher values = longer durations
fn build_ir_program() -> pio::Program<32> {
    let mut a = Assembler::<32>::new();

    let mut wait_start = a.label();
    let mut count_low = a.label();
    let mut low_done = a.label();
    let mut count_high = a.label();
    let mut cont_high = a.label();

    // Wait for pin LOW (start of IR pulse)
    a.bind(&mut wait_start);
    a.jmp(JmpCondition::PinHigh, &mut wait_start);

    // Pin is now LOW - start measuring pulse duration
    a.mov(MovDestination::X, MovOperation::Invert, MovSource::NULL);

    // Count while pin is low (pulse duration)
    a.bind(&mut count_low);
    a.jmp(JmpCondition::PinHigh, &mut low_done);
    a.jmp(JmpCondition::XDecNonZero, &mut count_low);

    a.bind(&mut low_done);
    a.mov(MovDestination::ISR, MovOperation::Invert, MovSource::X);
    a.push(false, false);

    // Now pin is high - count space duration
    a.mov(MovDestination::X, MovOperation::Invert, MovSource::NULL);

    a.bind(&mut count_high);
    a.jmp(JmpCondition::PinHigh, &mut cont_high);
    // Pin went low - done counting high, push and restart
    a.mov(MovDestination::ISR, MovOperation::Invert, MovSource::X);
    a.push(false, false);
    a.jmp(JmpCondition::Always, &mut wait_start);

    a.bind(&mut cont_high);
    a.jmp(JmpCondition::XDecNonZero, &mut count_high);
    // X reached zero (timeout) - push anyway and restart
    a.mov(MovDestination::ISR, MovOperation::Invert, MovSource::X);
    a.push(false, false);
    a.jmp(JmpCondition::Always, &mut wait_start);

    a.assemble_program()
}

/// NEC protocol decoder states
///
/// State machine sequence: LeaderPulse -> LeaderSpace -> (BitPulse -> BitSpace) x 32
#[derive(Clone, Copy, PartialEq)]
enum DecoderState {
    /// Waiting for 9ms LOW pulse that starts every NEC transmission
    LeaderPulse,
    /// Waiting for 4.5ms HIGH space after leader pulse
    LeaderSpace,
    /// Waiting for 562µs LOW pulse that starts each data bit
    BitPulse,
    /// Waiting for HIGH space that encodes bit value (562µs = 0, 1687µs = 1)
    BitSpace,
}

/// Internal NEC protocol decoder state machine
struct NecDecoder {
    state: DecoderState,
    bits_received: u8,
    data: u32,
}

impl NecDecoder {
    fn new() -> Self {
        Self {
            state: DecoderState::LeaderPulse,
            bits_received: 0,
            data: 0,
        }
    }

    /// Process a duration value from PIO
    /// is_pulse: true for pulse (pin low), false for space (pin high)
    fn process(&mut self, duration: u32, is_pulse: bool) -> Option<IrCommand> {
        match self.state {
            DecoderState::LeaderPulse => {
                if is_pulse && (timing::LEADER_PULSE_MIN..=timing::LEADER_PULSE_MAX).contains(&duration) {
                    self.state = DecoderState::LeaderSpace;
                }
            }
            DecoderState::LeaderSpace => {
                if !is_pulse && (timing::LEADER_SPACE_MIN..=timing::LEADER_SPACE_MAX).contains(&duration) {
                    self.state = DecoderState::BitPulse;
                    self.bits_received = 0;
                    self.data = 0;
                } else {
                    self.reset();
                }
            }
            DecoderState::BitPulse => {
                if is_pulse && (timing::BIT_PULSE_MIN..=timing::BIT_PULSE_MAX).contains(&duration) {
                    self.state = DecoderState::BitSpace;
                } else {
                    self.reset();
                }
            }
            DecoderState::BitSpace => {
                if !is_pulse {
                    let bit = if duration <= timing::BIT_ZERO_SPACE_MAX {
                        0
                    } else if (timing::BIT_ONE_SPACE_MIN..=timing::BIT_ONE_SPACE_MAX).contains(&duration) {
                        1
                    } else {
                        self.reset();
                        return None;
                    };

                    self.data = (self.data >> 1) | ((bit as u32) << 31);
                    self.bits_received += 1;

                    if self.bits_received == 32 {
                        let result = self.decode_command();
                        self.reset();
                        return result;
                    } else {
                        self.state = DecoderState::BitPulse;
                    }
                } else {
                    self.reset();
                }
            }
        }
        None
    }

    fn reset(&mut self) {
        self.state = DecoderState::LeaderPulse;
        self.bits_received = 0;
        self.data = 0;
    }

    fn decode_command(&self) -> Option<IrCommand> {
        let address = (self.data & 0xFF) as u8;
        let command = ((self.data >> 16) & 0xFF) as u8;
        let command_inv = ((self.data >> 24) & 0xFF) as u8;

        if command ^ command_inv == 0xFF {
            Some(IrCommand { address, command })
        } else {
            None
        }
    }
}

/// PIO-based NEC IR decoder
///
/// Owns the PIO RX FIFO and decoder state. Call `poll()` frequently
/// to drain the FIFO and decode commands.
pub struct NecIrDecoder {
    rx: Rx<(rp2040_hal::pac::PIO1, SM0)>,
    decoder: NecDecoder,
    is_pulse: bool,
}

impl NecIrDecoder {
    /// Initialize PIO for IR reception
    ///
    /// Configures the PIO state machine and starts it running.
    /// The returned decoder owns the RX FIFO.
    pub fn new(ir_pin: IrReceiverPin, mut pio: Pio1, sm: Pio1SM0) -> Self {
        // Reconfigure IR pin for PIO1
        let ir_pin_pio: IrReceiverPinPio = ir_pin.reconfigure();
        let pin_id = ir_pin_pio.id().num;

        // Install PIO program
        let program = build_ir_program();
        let installed = pio.install(&program).expect("PIO program install failed");

        // Configure state machine
        let (mut sm, rx, _tx) = PIOBuilder::from_installed_program(installed)
            .jmp_pin(pin_id)
            .in_pin_base(pin_id)
            .clock_divisor_fixed_point(PIO_CLOCK_DIVISOR, 0)
            .build(sm);

        sm.set_pindirs([(pin_id, PinDir::Input)]);
        sm.start();

        Self {
            rx,
            decoder: NecDecoder::new(),
            is_pulse: true,
        }
    }

    /// Poll for a decoded IR command (non-blocking)
    ///
    /// Drains available data from the PIO FIFO and decodes it.
    /// Returns `Some(IrCommand)` when a complete valid command is decoded.
    ///
    /// Call this frequently (every ~10-50ms) to avoid FIFO overflow.
    pub fn poll(&mut self) -> Option<IrCommand> {
        // Process all available FIFO entries
        while let Some(duration) = self.rx.read() {
            if let Some(cmd) = self.decoder.process(duration, self.is_pulse) {
                self.is_pulse = !self.is_pulse;
                return Some(cmd);
            }
            self.is_pulse = !self.is_pulse;
        }
        None
    }

    /// Check if there's data waiting in the FIFO
    pub fn has_data(&mut self) -> bool {
        !self.rx.is_empty()
    }

    /// Enable interrupt when RX FIFO is not empty
    pub fn enable_fifo_interrupt(&self) {
        self.rx.enable_rx_not_empty_interrupt(PioIRQ::Irq0);
    }

    /// Reset decoder state (call after long delays to clear stale data)
    pub fn reset(&mut self) {
        // Drain FIFO
        while self.rx.read().is_some() {}
        // Reset state
        self.decoder.reset();
        self.is_pulse = true;
    }
}
