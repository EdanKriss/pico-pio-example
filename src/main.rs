#![no_std]
#![no_main]

mod board;
mod buzzer;
mod main_core1;
mod ir_nec_pio;
mod oled;
mod rgb_led;

use rp2040_boot2::BOOT_LOADER_GENERIC_03H;
use panic_halt as _;
use cortex_m_rt::entry;
use fugit::ExtU32;
use smart_leds::RGB8;

use crate::buzzer::zelda_chest_sound;
use crate::rgb_led::{PicoBricksLedStepper, PicoBricksRgbLedSequence};

/**
    Link the second stage bootloader for RP2040 to the .boot2 section in memory.x
    Starting in Rust 2024, link_section requires unsafe wrapper
 */
#[unsafe(link_section = ".boot2")]
#[used]
static BOOT2: [u8; 256] = BOOT_LOADER_GENERIC_03H;

#[entry]
fn main() -> ! {
    let (mut board, core1_peripherals, mut ir_consumer) = board::Board::init();

    zelda_chest_sound(&mut board);

    main_core1::spawn(
        &mut board.multicore_peripherals,
        core1_peripherals,
    );

    let sequence: PicoBricksRgbLedSequence<5> = [
        [ RGB8::new(32, 0, 0) ],   // Red
        [ RGB8::new(0, 32, 0) ],   // Green
        [ RGB8::new(0, 0, 32) ],   // Blue
        [ RGB8::new(12, 12, 12) ], // White
        [ RGB8::new(0, 0, 0) ],    // Off
    ];

    let mut led_stepper = PicoBricksLedStepper::new(sequence, 300);

    board.watchdog.start(8_u32.secs());

    loop {
        while let Some(cmd) = ir_consumer.dequeue() {
            oled::show_command(&mut board.oled, cmd);
        }

        let _ = led_stepper.update(&mut board.rgb_led_chain);

        board.watchdog.feed();
    }
}
