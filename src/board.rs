use rp2040_hal as hal;
use hal::gpio::{FunctionPio0, FunctionSio, Pin, PullDown, PullUp, SioInput, SioOutput};
use hal::gpio::bank0::{Gpio0, Gpio6, Gpio7, Gpio20};
use hal::multicore::Stack;
use hal::pio::PIOExt;
use hal::sio::SioFifo;
use hal::Clock;
use ws2812_pio::Ws2812;

// PicoBricks board hardware configuration
pub type RgbLedPin = Pin<Gpio6, FunctionPio0, PullDown>;
pub type SimpleLedPin = Pin<Gpio7, FunctionSio<SioOutput>, PullDown>;
pub type BuzzerPin = Pin<Gpio20, FunctionSio<SioOutput>, PullDown>;
pub type IrReceiverPin = Pin<Gpio0, FunctionSio<SioInput>, PullUp>;

/// Each consumes 1 of 8 PIO state machines
pub type RgbLedChain = Ws2812<hal::pac::PIO0, hal::pio::SM0, hal::timer::CountDown, RgbLedPin>;

/// Stack for core1 - 4KB should be plenty for IR polling
pub static CORE1_STACK: Stack<4096> = Stack::new();

/// Peripherals owned by core0 context
pub struct Board {
    pub timer: hal::Timer,
    pub watchdog: hal::Watchdog,
    pub rgb_led_chain: RgbLedChain,
    pub buzzer: BuzzerPin,
    pub mc: MulticorePeripherals,
}

/// Peripherals needed to spawn core1 via Multicore::new
pub struct MulticorePeripherals {
    /// Hardware FIFO queue for inter-core communication
    pub sio_fifo: SioFifo,
    pub psm: hal::pac::PSM,
    pub ppb: hal::pac::PPB,
}

/// Peripherals owned by core1 context
pub struct BoardCore1 {
    pub ir_receiver: IrReceiverPin,
    pub simple_led: SimpleLedPin,
}

impl Board {
    pub fn init() -> (Self, BoardCore1) {
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

        let sio = hal::Sio::new(pac.SIO);

        let pins = hal::gpio::Pins::new(
            pac.IO_BANK0,
            pac.PADS_BANK0,
            sio.gpio_bank0,
            &mut pac.RESETS,
        );

        let timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

        let (
            mut pio,
            sm0,
            _, _, _
        ) = pac.PIO0.split(&mut pac.RESETS);

        let rgb_led_chain = Ws2812::new(
            pins.gpio6.into_function(),
            &mut pio,
            sm0,
            clocks.peripheral_clock.freq(),
            timer.count_down(),
        );

        let simple_led = pins.gpio7.into_push_pull_output();

        let buzzer = pins.gpio20.into_push_pull_output();

        let ir_receiver = pins.gpio0.into_pull_up_input();

        let mc = MulticorePeripherals {
            sio_fifo: sio.fifo,
            psm: pac.PSM,
            ppb: pac.PPB,
        };

        let board = Self {
            timer,
            watchdog,
            rgb_led_chain,
            buzzer,
            mc,
        };

        let board_core1 = BoardCore1 {
            ir_receiver,
            simple_led,
        };

        (board, board_core1)
    }
}
