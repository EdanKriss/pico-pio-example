#![no_std]
#![no_main]

mod board;
mod buzzer;
mod rgb_led;

use rp2040_boot2::BOOT_LOADER_GENERIC_03H;
use panic_halt as _;
use cortex_m_rt::entry;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use smart_leds::{RGB8};
use fugit::ExtU32;

use crate::board::Board;
use crate::buzzer::{error_beep, zelda_chest_sound};
use crate::rgb_led::{PicoBricksRgbLedSequence, simple_ws2812_sequence};

/**
 *  Link the second stage bootloader for RP2040 to the .boot2 section in memory.x
 *  In Rust 2024, link_section requires unsafe wrapper
 */
#[unsafe(link_section = ".boot2")]
#[used]
static BOOT2: [u8; 256] = BOOT_LOADER_GENERIC_03H;

#[entry]
fn main() -> ! {
    let mut board = Board::init();
    zelda_chest_sound(&mut board);

    let sequence: PicoBricksRgbLedSequence<4> = [
        [ RGB8::new(32, 0, 0) ], // Red
        [ RGB8::new(0, 32, 0) ], // Green
        [ RGB8::new(0, 0, 32) ], // Blue
        [ RGB8::new(0, 0, 0) ], // Off
    ];

    board.watchdog.start(8_u32.secs());

    loop {
        if simple_ws2812_sequence(
            &sequence,
            300,
            &mut board.rgb_led,
            &mut board.timer
        ).is_err() 
        {
            error_beep(&mut board);
        }

        // Simple LED blink
        let _ = board.simple_led.set_high();
        board.timer.delay_ms(500);
        let _ = board.simple_led.set_low();
        board.timer.delay_ms(500);

        board.watchdog.feed();
    }
}
