#![no_std]
#![no_main]

mod board;

use rp2040_boot2::BOOT_LOADER_GENERIC_03H;
use panic_halt as _;
use cortex_m_rt::entry;
use embedded_hal::digital::OutputPin;
use smart_leds::{brightness, SmartLedsWrite, RGB8};
use fugit::ExtU32;

use board::Board;

// Link the second stage bootloader for RP2040 to the .boot2 section in memory.x                                                                     
// In Rust 2024, link_section requires unsafe wrapper   
#[unsafe(link_section = ".boot2")]
#[used]
static BOOT2: [u8; 256] = BOOT_LOADER_GENERIC_03H;

fn play_tone(
    Board {
        delay,
        buzzer,
        ..
    }: &mut Board,
    freq_hz: u32,
    duration_ms: u32,
) {
    if freq_hz == 0 {
        delay.delay_ms(duration_ms);
        return;
    }

    let half_period_us = 500_000 / freq_hz;
    let cycles = (duration_ms * 1000) / (half_period_us * 2);

    for _ in 0..cycles {
        let _ = buzzer.set_high();
        delay.delay_us(half_period_us);
        let _ = buzzer.set_low();
        delay.delay_us(half_period_us);
    }
}

fn error_beep(board: &mut Board) {
    play_tone(board, 2000, 200);
}

fn zelda_chest_sound(board: &mut Board) {
    // Note frequencies
    const A4: u32 = 440;
    const CS5: u32 = 554;
    const E5: u32 = 659;
    const A5: u32 = 880;

    // Quick ascending arpeggio
    play_tone(board, A4, 100);
    play_tone(board, CS5, 100);
    play_tone(board, E5, 100);
    // Triumphant sustained note
    play_tone(board, A5, 600);
}

#[entry]
fn main() -> ! {
    let mut board = Board::init();
    zelda_chest_sound(&mut board);

    let mut leds = [RGB8::default(); 1];

    board.watchdog.start(8_u32.secs());

    loop {
        // Red
        leds[0] = RGB8::new(255, 0, 0);
        if board.rgb_led.write(brightness(leds.iter().copied(), 32)).is_err() {
            error_beep(&mut board);
        }
        board.delay.delay_ms(1000);

        // Green
        leds[0] = RGB8::new(0, 255, 0);
        if board.rgb_led.write(brightness(leds.iter().copied(), 32)).is_err() {
            error_beep(&mut board);
        }
        board.delay.delay_ms(1000);

        // Blue
        leds[0] = RGB8::new(0, 0, 255);
        if board.rgb_led.write(brightness(leds.iter().copied(), 32)).is_err() {
            error_beep(&mut board);
        }
        board.delay.delay_ms(1000);

        // Off
        leds[0] = RGB8::new(0, 0, 0);
        if board.rgb_led.write(leds.iter().copied()).is_err() {
            error_beep(&mut board);
        }
        board.delay.delay_ms(1000);

        // Simple LED blink
        let _ = board.simple_led.set_high();
        board.delay.delay_ms(500);
        let _ = board.simple_led.set_low();
        board.delay.delay_ms(500);

        board.watchdog.feed();
    }
}
