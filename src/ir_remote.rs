//! Simple NEC IR protocol decoder using polling
//!
//! NEC Protocol timing (at 38kHz carrier):
//! - Leader: 9ms pulse + 4.5ms space
//! - Repeat: 9ms pulse + 2.25ms space
//! - Bit 0: 562.5µs pulse + 562.5µs space
//! - Bit 1: 562.5µs pulse + 1.6875ms space
//! - 32 bits total: address (8) + ~address (8) + command (8) + ~command (8)

/// Sample period in microseconds - poll the IR receiver at this rate
pub const SAMPLE_PERIOD_US: u32 = 50;

/// Timing thresholds in sample counts (at 50µs per sample)
const LEADER_PULSE_MIN: u16 = 160;   // 8ms minimum for leader pulse
const LEADER_PULSE_MAX: u16 = 200;   // 10ms maximum
const LEADER_SPACE_MIN: u16 = 80;    // 4ms minimum for leader space
const LEADER_SPACE_MAX: u16 = 100;   // 5ms maximum
const BIT_PULSE_MIN: u16 = 8;        // 400µs minimum for bit pulse
const BIT_PULSE_MAX: u16 = 16;       // 800µs maximum
const BIT_ZERO_SPACE_MAX: u16 = 16;  // 800µs - space shorter than this = 0
const BIT_ONE_SPACE_MIN: u16 = 20;   // 1ms - space longer than this = 1
const BIT_ONE_SPACE_MAX: u16 = 40;   // 2ms maximum

#[derive(Clone, Copy, PartialEq)]
enum State {
    Idle,
    LeaderPulse,
    LeaderSpace,
    BitPulse,
    BitSpace,
}

/// Decoded NEC command with address and command bytes
#[derive(Debug, Clone, Copy)]
pub struct IrCommand {
    pub address: u8,
    pub command: u8,
}

/// NEC IR receiver using polling-based decoding
pub struct IrRemote {
    state: State,
    sample_count: u16,
    bits_received: u8,
    data: u32,
    last_pin_state: bool,
}

impl IrRemote {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            sample_count: 0,
            bits_received: 0,
            data: 0,
            last_pin_state: false,
        }
    }

    /// Poll with raw pin state (true = IR signal detected, i.e., pin is low)
    /// Returns Some(IrCommand) when a complete valid command is decoded.
    pub fn poll_raw(&mut self, ir_active: bool) -> Option<IrCommand> {
        let edge = ir_active != self.last_pin_state;
        self.last_pin_state = ir_active;

        if edge {
            let count = self.sample_count;
            self.sample_count = 0;

            match self.state {
                State::Idle => {
                    if ir_active {
                        // Rising edge - start of leader pulse
                        self.state = State::LeaderPulse;
                    }
                }
                State::LeaderPulse => {
                    if !ir_active {
                        // Falling edge - end of leader pulse
                        if (LEADER_PULSE_MIN..=LEADER_PULSE_MAX).contains(&count) {
                            self.state = State::LeaderSpace;
                        } else {
                            self.reset();
                        }
                    }
                }
                State::LeaderSpace => {
                    if ir_active {
                        // Rising edge - end of leader space
                        if (LEADER_SPACE_MIN..=LEADER_SPACE_MAX).contains(&count) {
                            self.state = State::BitPulse;
                            self.bits_received = 0;
                            self.data = 0;
                        } else {
                            self.reset();
                        }
                    }
                }
                State::BitPulse => {
                    if !ir_active {
                        // Falling edge - end of bit pulse
                        if (BIT_PULSE_MIN..=BIT_PULSE_MAX).contains(&count) {
                            self.state = State::BitSpace;
                        } else {
                            self.reset();
                        }
                    }
                }
                State::BitSpace => {
                    if ir_active {
                        // Rising edge - end of bit space, decode the bit
                        let bit = if count <= BIT_ZERO_SPACE_MAX {
                            0
                        } else if (BIT_ONE_SPACE_MIN..=BIT_ONE_SPACE_MAX).contains(&count) {
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
                            self.state = State::BitPulse;
                        }
                    }
                }
            }
        } else {
            // No edge - increment sample counter (with saturation)
            self.sample_count = self.sample_count.saturating_add(1);

            // Timeout: if we're waiting for an edge too long, reset
            if self.sample_count > 300 && self.state != State::Idle {
                self.reset();
            }
        }

        None
    }

    fn reset(&mut self) {
        self.state = State::Idle;
        self.sample_count = 0;
        self.bits_received = 0;
        self.data = 0;
    }

    fn decode_command(&self) -> Option<IrCommand> {
        // NEC format: address (8) + ~address (8) + command (8) + ~command (8)
        let address = (self.data & 0xFF) as u8;
        let _address_inv = ((self.data >> 8) & 0xFF) as u8;
        let command = ((self.data >> 16) & 0xFF) as u8;
        let command_inv = ((self.data >> 24) & 0xFF) as u8;

        // Validate: inverted bytes should be complement of original
        // Some remotes use extended NEC (16-bit address), so only validate command
        if command ^ command_inv == 0xFF {
            Some(IrCommand { address, command })
        } else {
            None
        }
    }
}

impl Default for IrRemote {
    fn default() -> Self {
        Self::new()
    }
}

/// Known button codes for PicoBricks remote (to be filled in after discovery)
/// These are placeholder values - run the discovery program to find actual codes
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
