use rp2040_hal as hal;
use hal::gpio::{FunctionPio0, FunctionSio, Pin, PullDown, SioOutput};
use hal::gpio::bank0::{Gpio6, Gpio7, Gpio20};
use hal::pio::SM0;
use hal::Clock;
use ws2812_pio::Ws2812;

// PicoBricks hardware configuration
pub type SimpleLedPin = Pin<Gpio7, FunctionSio<SioOutput>, PullDown>;
pub type BuzzerPin = Pin<Gpio20, FunctionSio<SioOutput>, PullDown>;
pub type RgbPin = Pin<Gpio6, FunctionPio0, PullDown>;
pub type RgbLed = Ws2812<hal::pac::PIO0, SM0, hal::timer::CountDown, RgbPin>;

pub struct Board {
    pub delay: cortex_m::delay::Delay,
    pub watchdog: hal::Watchdog,
    pub rgb_led: RgbLed,
    pub simple_led: SimpleLedPin,
    pub buzzer: BuzzerPin,
}

impl Board {
    pub fn init() -> Self {
        let core = hal::pac::CorePeripherals::take().unwrap();
        let mut pac = hal::pac::Peripherals::take().unwrap();

        let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);

        let clocks = hal::clocks::init_clocks_and_plls(
            12_000_000u32,
            pac.XOSC,
            pac.CLOCKS,
            pac.PLL_SYS,
            pac.PLL_USB,
            &mut pac.RESETS,
            &mut watchdog,
        )
        .expect("clock init failed");

        let sio = hal::Sio::new(pac.SIO);
        let pins = hal::gpio::Pins::new(
            pac.IO_BANK0,
            pac.PADS_BANK0,
            sio.gpio_bank0,
            &mut pac.RESETS,
        );

        let delay = cortex_m::delay::Delay::new(
            core.SYST,
            clocks.system_clock.freq().to_Hz(),
        );

        let timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

        use hal::pio::PIOExt;
        let (mut pio, sm0, _, _, _) = pac.PIO0.split(&mut pac.RESETS);

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
            delay,
            watchdog,
            rgb_led,
            simple_led,
            buzzer,
        }
    }
}
