use rp2040_hal as hal;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;

use crate::board::BuzzerPin;

fn play_tone(
    timer: &mut hal::Timer,
    buzzer: &mut BuzzerPin,
    freq_hz: u32,
    duration_ms: u32,
) {
    // Return to avoid divide by zero
    if freq_hz == 0 {
        timer.delay_ms(duration_ms);
        return;
    }

    let half_period_us = 500_000 / freq_hz;
    let cycles = (duration_ms * 1000) / (half_period_us * 2);

    for _ in 0..cycles {
        let _ = buzzer.set_high();
        timer.delay_us(half_period_us);
        let _ = buzzer.set_low();
        timer.delay_us(half_period_us);
    }
}

pub fn error_beep(
    timer: &mut hal::Timer,
    buzzer: &mut BuzzerPin,
) {
    // Descending minor third, like "uh-oh"
    const HIGH: u32 = 400;
    const LOW: u32 = 320;

    play_tone(timer, buzzer, HIGH, 150);
    timer.delay_ms(50);
    play_tone(timer, buzzer, LOW, 300);
}

pub fn zelda_chest_sound(
    timer: &mut hal::Timer,
    buzzer: &mut BuzzerPin,
) {
    // Note frequencies
    const A4: u32 = 440;
    const CS5: u32 = 554;
    const E5: u32 = 659;
    const A5: u32 = 880;

    // Quick ascending arpeggio
    play_tone(timer, buzzer, A4, 100);
    play_tone(timer, buzzer, CS5, 100);
    play_tone(timer, buzzer, E5, 100);
    // Triumphant sustained note
    play_tone(timer, buzzer, A5, 600);
}
