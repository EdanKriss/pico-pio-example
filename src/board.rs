use rp2040_hal as hal;
use hal::gpio::{FunctionPio0, FunctionSio, Pin, PullDown, SioOutput};
use hal::gpio::bank0::{Gpio6, Gpio7, Gpio20};
use hal::pio::PIOExt;
use hal::Clock;
use ws2812_pio::Ws2812;

// PicoBricks hardware configuration
pub type RgbPin = Pin<Gpio6, FunctionPio0, PullDown>;
pub type SimpleLedPin = Pin<Gpio7, FunctionSio<SioOutput>, PullDown>;
pub type BuzzerPin = Pin<Gpio20, FunctionSio<SioOutput>, PullDown>;
pub type RgbLed = Ws2812<hal::pac::PIO0, hal::pio::SM0, hal::timer::CountDown, RgbPin>;

pub struct Board {
    pub timer: hal::Timer,
    pub watchdog: hal::Watchdog,
    pub rgb_led: RgbLed,
    pub simple_led: SimpleLedPin,
    pub buzzer: BuzzerPin,
}

impl Board {
    pub fn init() -> Self {
        let mut pac = hal::pac::Peripherals::take().unwrap();

        let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);

        let clocks = hal::clocks::init_clocks_and_plls(
            12_000_000u32,
            pac.XOSC,
            pac.CLOCKS,
            pac.PLL_SYS,
            pac.PLL_USB,
            &mut pac.RESETS,
            &mut watchdog, // quirk of the RP2040: clocks require a watchdog because it owns the Tick generator
        )
        .expect("ClocksManager init failed");

        let pins = hal::gpio::Pins::new(
            pac.IO_BANK0,
            pac.PADS_BANK0,
            hal::Sio::new(pac.SIO).gpio_bank0,
            &mut pac.RESETS,
        );

        let timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

        let (
            mut pio,
            sm0,
            _, _, _
        ) = pac.PIO0.split(&mut pac.RESETS);

        let rgb_led = Ws2812::new(
            pins.gpio6.into_function(),
            &mut pio,
            sm0,
            clocks.peripheral_clock.freq(),
            timer.count_down(),
        );

        let simple_led = pins.gpio7.into_push_pull_output();

        let buzzer = pins.gpio20.into_push_pull_output();

        Self {
            timer,
            watchdog,
            rgb_led,
            simple_led,
            buzzer,
        }
    }
}
