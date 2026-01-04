use embedded_hal::delay::DelayNs;
use smart_leds::{SmartLedsWrite, RGB8};

/**
 *  One element for each LED in chain.
 *  This is the type to pass to SmartLedsWrite::write()
 */
type LedChainUpdate<const LED_CHAIN_LENGTH: usize> =
    [RGB8; LED_CHAIN_LENGTH];

/**
 *  One element for each LedChainUpdate, corresponding to steps in a sequence
 */
type LedChainSequence<const LED_CHAIN_LENGTH: usize, const SEQUENCE_LENGTH: usize> =
    [LedChainUpdate<LED_CHAIN_LENGTH>; SEQUENCE_LENGTH];

/**
 *  PicoBricks built-in LED "chain" only has one Ws2812 LED.
 */
pub type PicoBricksRgbLedSequence<const SEQUENCE_LENGTH: usize> =
    LedChainSequence<1, SEQUENCE_LENGTH>;

/**
 *  Iterates through color_sequence, setting LED chain with each update element
 *  in order, waiting step_duration ms after each.
 *
 *  Works with any addressable LED chain driver instance that implements 
 *  SmartLedsWrite<Color = RGB8>, such as WS2812, APA102, SK6812 (RGB mode)
 */
pub fn smart_leds_simple_sequence<
    const LED_CHAIN_LENGTH: usize,
    const SEQUENCE_LENGTH: usize,
    LedDriver,
    Timer
>(
    color_sequence: &LedChainSequence<LED_CHAIN_LENGTH, SEQUENCE_LENGTH>,
    step_duration: u32,
    led_chain: &mut LedDriver,
    timer: &mut Timer,
) -> Result<(), LedDriver::Error>
where
    LedDriver: SmartLedsWrite<Color = RGB8>,
    Timer: DelayNs,
{
    for update in color_sequence {
        led_chain.write(update.iter().copied())?;
        timer.delay_ms(step_duration);
    }
    Ok(())
}
