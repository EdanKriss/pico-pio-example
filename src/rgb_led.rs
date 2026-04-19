use smart_leds::{SmartLedsWrite, RGB8};

use crate::board::read_timer_us;

/**
 *  One element for each LED in chain.
 *  This is the type passed to SmartLedsWrite::write()
 */
type LedChainUpdate<const LED_CHAIN_LENGTH: usize> =
    [RGB8; LED_CHAIN_LENGTH];

/**
 *  One element for each LedChainUpdate, corresponding to steps in a sequence.
 */
type LedChainSequence<const LED_CHAIN_LENGTH: usize, const SEQUENCE_LENGTH: usize> =
    [LedChainUpdate<LED_CHAIN_LENGTH>; SEQUENCE_LENGTH];

/**
 *  PicoBricks built-in LED "chain" only has one Ws2812 LED.
 */
pub type PicoBricksRgbLedSequence<const SEQUENCE_LENGTH: usize> =
    LedChainSequence<1, SEQUENCE_LENGTH>;

/**
 *  Non-blocking sequence stepper.
 *
 *  Owns the colour sequence and the timestamp (microseconds since boot) at
 *  which the current step was last written. `update()` returns immediately
 *  unless enough time has elapsed to advance. The first call draws step 0 so
 *  the LED is never left dark waiting out the initial interval.
 *
 *  Works with any addressable LED chain driver that implements
 *  SmartLedsWrite<Color = RGB8> (WS2812, APA102, SK6812 RGB, ...).
 */
pub struct LedSequenceStepper<const LED_CHAIN_LENGTH: usize, const SEQUENCE_LENGTH: usize> {
    sequence: LedChainSequence<LED_CHAIN_LENGTH, SEQUENCE_LENGTH>,
    step_duration_us: u32,
    current_step: usize,
    last_step_us: u32,
    first_update: bool,
}

pub type PicoBricksLedStepper<const SEQUENCE_LENGTH: usize> =
    LedSequenceStepper<1, SEQUENCE_LENGTH>;

impl<const LED_CHAIN_LENGTH: usize, const SEQUENCE_LENGTH: usize>
    LedSequenceStepper<LED_CHAIN_LENGTH, SEQUENCE_LENGTH>
{
    pub fn new(
        sequence: LedChainSequence<LED_CHAIN_LENGTH, SEQUENCE_LENGTH>,
        step_duration_ms: u32,
    ) -> Self {
        Self {
            sequence,
            step_duration_us: step_duration_ms.saturating_mul(1_000),
            current_step: 0,
            last_step_us: 0,
            first_update: true,
        }
    }

    pub fn update<LedDriver>(&mut self, led_chain: &mut LedDriver) -> Result<(), LedDriver::Error>
    where
        LedDriver: SmartLedsWrite<Color = RGB8>,
    {
        let now = read_timer_us();

        if self.first_update {
            self.first_update = false;
            self.last_step_us = now;
            return led_chain.write(self.sequence[self.current_step].iter().copied());
        }

        // wrapping_sub tolerates the 32-bit microsecond timer rollover (~71 min).
        if now.wrapping_sub(self.last_step_us) < self.step_duration_us {
            return Ok(());
        }

        self.current_step = (self.current_step + 1) % SEQUENCE_LENGTH;
        self.last_step_us = now;
        led_chain.write(self.sequence[self.current_step].iter().copied())
    }
}
