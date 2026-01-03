use smart_leds::{SmartLedsWrite, RGB8};

use crate::board::{Board};
use crate::buzzer::{error_beep};

// One element for each LED in chain.
// PicoBricks built-in LED "chain" only has one Ws2812 LED.
pub type LedChainUpdate = [RGB8; 1];

pub fn simple_ws2812_sequence(
    // color_sequence: impl IntoIterator<Item = LedChainUpdate>,
    color_sequence: &[LedChainUpdate],
    step_duration: u32,
    board: &mut Board,
) {
    for update in color_sequence {
        if board.rgb_led.write(*update).is_err() {
            error_beep(board);
        }
        board.delay.delay_ms(step_duration);
    }
}
