#![no_std]
#![no_main]

use rp2040_boot2::BOOT_LOADER_GENERIC_03H;
use panic_halt as _;
use cortex_m_rt::entry;
use rp2040_hal as hal;
use hal::Clock;
use embedded_hal::digital::OutputPin;
use smart_leds::{brightness, SmartLedsWrite, RGB8};
use ws2812_pio::Ws2812;
use fugit::ExtU32;

// Link the second stage bootloader for RP2040 to the .boot2 section in memory.x 
// In Rust 2024, link_section requires unsafe wrapper
#[unsafe(link_section = ".boot2")]
#[used]
static BOOT2: [u8; 256] = BOOT_LOADER_GENERIC_03H;

#[entry]
fn main() -> ! {
    let core = hal::pac::CorePeripherals::take().unwrap();
    let mut pac = hal::pac::Peripherals::take().unwrap();

    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);

    let clocks = hal::clocks::init_clocks_and_plls(
        12_000_000u32, // 12 MHz crystal oscillator frequency, can import this value from 'use rp_pico::XOSC_CRYSTAL_FREQ'
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .expect("clock init failed");

    // Set up the GPIO pins using the Single Cycle I/O (SIO) block
    let sio = hal::Sio::new(pac.SIO);
    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    let mut delay = cortex_m::delay::Delay::new(
        core.SYST,
        clocks.system_clock.freq().to_Hz(),
    );

    let timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

    // Set up the PIO (Programmable I/O) peripheral for WS2812 control
    // The PIO is a specialized coprocessor that can generate precise timing
    use hal::pio::PIOExt;
    let (mut pio, sm0, _, _, _) = pac.PIO0.split(&mut pac.RESETS);

    // Configure GPIO 6 for the WS2812 RGB LED
    // WS2812 LEDs need precise timing that the PIO peripheral provides
    let mut rgb_led = Ws2812::new(
        pins.gpio6.into_function(),
        &mut pio,
        sm0,
        clocks.peripheral_clock.freq(),
        timer.count_down(),
    );

    // Keep GPIO 7 for the simple LED blinking
    let mut simple_led = pins.gpio7.into_push_pull_output();

    let mut error_led = pins.gpio8.into_push_pull_output();

    let mut leds = [RGB8::default(); 1];

    watchdog.start(8_u32.secs());

    loop {
        /*
         * Smart LED
         */

        // Red color
        leds[0] = RGB8::new(255, 0, 0);
        if rgb_led.write(brightness(leds.iter().copied(), 32)).is_err() {
            error_led.set_high().unwrap();
        }
        delay.delay_ms(1000);

        // Green color
        leds[0] = RGB8::new(0, 255, 0);
        if rgb_led.write(brightness(leds.iter().copied(), 32)).is_err() {
            error_led.set_high().unwrap();
        }
        delay.delay_ms(1000);

        // Blue color
        leds[0] = RGB8::new(0, 0, 255);
        if rgb_led.write(brightness(leds.iter().copied(), 32)).is_err() {
            error_led.set_high().unwrap();
        }
        delay.delay_ms(1000);

        // Turn off
        leds[0] = RGB8::new(0, 0, 0);
        if rgb_led.write(leds.iter().copied()).is_err() {
            error_led.set_high().unwrap();
        }
        delay.delay_ms(1000);

        /*
         * Simple LED
         */

        // Blink on then off
        simple_led.set_high().unwrap();
        delay.delay_ms(500);
        simple_led.set_low().unwrap();
        delay.delay_ms(500);

        watchdog.feed();
    }
}
