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

use pio::{Assembler, InSource, JmpCondition, MovDestination, MovOperation, MovSource, SetDestination};
use rp2040_hal::pio::{PIOBuilder, PinDir, PioIRQ, Rx, ShiftDirection, SM0};

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
    // NEC repeat-frame space: 2.25ms = 1125 counts (half of the leader space).
    // A repeat frame is 9ms pulse + 2.25ms space + 562µs pulse, sent ~110ms
    // after the initial command and every ~110ms while the button is held.
    pub const REPEAT_SPACE_MIN: u32 = 900;
    pub const REPEAT_SPACE_MAX: u32 = 1300;
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

/// Known button codes for the PicoBricks remote.
///
/// Generates named `pub const NAME: u8 = VALUE` declarations and a reverse lookup
/// via `buttons::name_of(cmd: u8) -> &'static str` from the buttons! invocation.
macro_rules! buttons {
    ($($name:ident = $value:literal),* $(,)?) => {
        #[allow(dead_code)]
        pub mod buttons {
            $(pub const $name: u8 = $value;)*

            /// Returns the button label for a NEC command byte, or
            /// `"UNKNOWN"` for codes not on the PicoBricks remote.
            pub fn name_of(cmd: u8) -> &'static str {
                match cmd {
                    $($value => stringify!($name),)*
                    _ => "UNKNOWN",
                }
            }
        }
    };
}

buttons! {
    KEY_0 = 0x19,
    KEY_1 = 0x45,
    KEY_2 = 0x46,
    KEY_3 = 0x47,
    KEY_4 = 0x44,
    KEY_5 = 0x40,
    KEY_6 = 0x43,
    KEY_7 = 0x07,
    KEY_8 = 0x15,
    KEY_9 = 0x09,
    KEY_STAR = 0x16,
    KEY_HASH = 0x0D,
    KEY_UP = 0x18,
    KEY_DOWN = 0x52,
    KEY_LEFT = 0x08,
    KEY_RIGHT = 0x5A,
    KEY_OK = 0x1C,
}

/// Build the PIO program for IR pulse/space measurement
///
/// Each pushed FIFO word is `(count << 1) | polarity`, where polarity is 0
/// for a pulse (pin low) and 1 for a space (pin high). Encoding polarity in
/// the word itself lets software stay in sync even if a FIFO entry is dropped.
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

    // Pin is now LOW - polarity = 0 (pulse), start measuring
    a.set(SetDestination::Y, 0);
    a.mov(MovDestination::X, MovOperation::Invert, MovSource::NULL);

    // Count while pin is low (pulse duration)
    a.bind(&mut count_low);
    a.jmp(JmpCondition::PinHigh, &mut low_done);
    a.jmp(JmpCondition::XDecNonZero, &mut count_low);

    a.bind(&mut low_done);
    // ISR = count (mov resets input shift counter to 0)
    a.mov(MovDestination::ISR, MovOperation::Invert, MovSource::X);
    // Shift polarity bit into ISR LSB: ISR = (count << 1) | 0
    a.r#in(InSource::Y, 1);
    a.push(false, false);

    // Pin is now HIGH - polarity = 1 (space), start measuring
    a.set(SetDestination::Y, 1);
    a.mov(MovDestination::X, MovOperation::Invert, MovSource::NULL);

    a.bind(&mut count_high);
    a.jmp(JmpCondition::PinHigh, &mut cont_high);
    // Pin went low - done counting space, push and restart
    a.mov(MovDestination::ISR, MovOperation::Invert, MovSource::X);
    a.r#in(InSource::Y, 1);
    a.push(false, false);
    a.jmp(JmpCondition::Always, &mut wait_start);

    a.bind(&mut cont_high);
    a.jmp(JmpCondition::XDecNonZero, &mut count_high);
    // X reached zero (timeout) - push anyway and restart
    a.mov(MovDestination::ISR, MovOperation::Invert, MovSource::X);
    a.r#in(InSource::Y, 1);
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
    /// Most recently decoded command. Re-emitted when a NEC repeat frame is
    /// seen (9ms pulse + 2.25ms space) so holding a remote button produces a
    /// stream of the same IrCommand every ~110ms.
    last_command: Option<IrCommand>,
}

impl NecDecoder {
    fn new() -> Self {
        Self {
            state: DecoderState::LeaderPulse,
            bits_received: 0,
            data: 0,
            last_command: None,
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
                } else if !is_pulse
                    && (timing::REPEAT_SPACE_MIN..=timing::REPEAT_SPACE_MAX).contains(&duration)
                {
                    // NEC repeat frame: no data follows — re-emit the last
                    // decoded command. The trailing 562µs pulse is silently
                    // absorbed by LeaderPulse (doesn't match its timing range).
                    //
                    // TODO: configurable repeat-debounce interval.
                    // Native NEC repeats fire every ~110ms, which is almost
                    // certainly too fast for user-facing scroll/hold actions
                    // (~9Hz). Add a minimum interval between emitted repeats
                    // (e.g. 250ms default, caller-overridable) so holding a
                    // button produces usable step rates. Needs a time source
                    // (`board::read_timer_us`) threaded into the decoder, or
                    // done at the consumer layer instead. Revisit once there
                    // is a scrollable UI to tune against.
                    self.state = DecoderState::LeaderPulse;
                    return self.last_command;
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
                        // Remember the last *valid* command so repeat frames
                        // can re-emit it; a bad checksum leaves the previous
                        // held command intact rather than overwriting with None.
                        if result.is_some() {
                            self.last_command = result;
                        }
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

        // Configure state machine. Shift LEFT so `in y, 1` after `mov isr, ~x`
        // lands the polarity bit at ISR[0], producing `(count << 1) | polarity`.
        let (mut sm, rx, _tx) = PIOBuilder::from_installed_program(installed)
            .jmp_pin(pin_id)
            .in_pin_base(pin_id)
            .in_shift_direction(ShiftDirection::Left)
            .clock_divisor_fixed_point(PIO_CLOCK_DIVISOR, 0)
            .build(sm);

        sm.set_pindirs([(pin_id, PinDir::Input)]);
        sm.start();

        Self {
            rx,
            decoder: NecDecoder::new(),
        }
    }

    /// Poll for a decoded IR command (non-blocking)
    ///
    /// Drains available data from the PIO FIFO and decodes it.
    /// Returns `Some(IrCommand)` when a complete valid command is decoded.
    /// When `None` is returned, the FIFO has been fully drained (so a
    /// level-triggered FIFO-not-empty IRQ will deassert).
    pub fn poll(&mut self) -> Option<IrCommand> {
        // Each FIFO word is (count << 1) | polarity, where polarity 0 = pulse
        while let Some(word) = self.rx.read() {
            let is_pulse = (word & 1) == 0;
            let duration = word >> 1;
            if let Some(cmd) = self.decoder.process(duration, is_pulse) {
                return Some(cmd);
            }
        }
        None
    }

    /// Enable the RX-FIFO-not-empty interrupt on PIO IRQ 0.
    /// Level-triggered: deasserts automatically once the FIFO is fully drained.
    pub fn enable_fifo_interrupt(&self) {
        self.rx.enable_rx_not_empty_interrupt(PioIRQ::Irq0);
    }
}
