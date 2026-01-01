// trait required to call set_low and set_high
use embedded_hal::digital::OutputPin;

use crate::board::Board;

fn play_tone(
    Board {
        delay,
        buzzer,
        ..
    }: &mut Board,
    freq_hz: u32,
    duration_ms: u32,
) {
    // return to avoid divide by zero
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

pub fn error_beep(board: &mut Board) {
    // Descending minor third - "uh-oh"
    const HIGH: u32 = 400;
    const LOW: u32 = 320;

    play_tone(board, HIGH, 150);
    board.delay.delay_ms(50);
    play_tone(board, LOW, 300);
}

pub fn zelda_chest_sound(board: &mut Board) {
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
