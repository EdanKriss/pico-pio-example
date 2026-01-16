use core::cell::RefCell;

use rp2040_hal as hal;
use hal::gpio::{FunctionPio0, FunctionPio1, FunctionSio, Pin, PullDown, PullUp, SioInput, SioOutput};
use hal::gpio::bank0::{Gpio0, Gpio6, Gpio7, Gpio20};
use hal::multicore::Stack;
use hal::pio::PIOExt;
use hal::sio::SioFifo;
use hal::Clock;
use critical_section::Mutex;
use ws2812_pio::Ws2812;

// PicoBricks board hardware configuration
pub type RgbLedPin = Pin<Gpio6, FunctionPio0, PullDown>;
pub type SimpleLedPin = Pin<Gpio7, FunctionSio<SioOutput>, PullDown>;
pub type BuzzerPin = Pin<Gpio20, FunctionSio<SioOutput>, PullDown>;

/// IR pin configured for GPIO input (before PIO reconfiguration)
pub type IrReceiverPin = Pin<Gpio0, FunctionSio<SioInput>, PullUp>;

/// IR pin configured for PIO1
pub type IrReceiverPinPio = Pin<Gpio0, FunctionPio1, PullUp>;

/// WS2812 LED chain on PIO0
pub type RgbLedChain = Ws2812<hal::pac::PIO0, hal::pio::SM0, hal::timer::CountDown, RgbLedPin>;

/// PIO types for specific blocks and state machines
pub type Pio1 = hal::pio::PIO<hal::pac::PIO1>;
pub type Pio1SM0 = hal::pio::UninitStateMachine<(hal::pac::PIO1, hal::pio::SM0)>;

/// Timer alarm type for core1 housekeeping
pub type Alarm1 = hal::timer::Alarm1;

/// Peripherals owned by core0
pub struct Board {
    pub timer: hal::Timer,
    pub watchdog: hal::Watchdog,
    pub rgb_led_chain: RgbLedChain,
    pub buzzer: BuzzerPin,
    pub multicore_peripherals: MulticorePeripherals,
}

/// Peripherals needed to spawn core1 via Multicore::new
pub struct MulticorePeripherals {
    pub sio_fifo: SioFifo,
    pub psm: hal::pac::PSM,
    pub ppb: hal::pac::PPB,
}

/// Peripherals to be transferred to core1
pub struct BoardCore1 {
    pub ir_receiver_pin: IrReceiverPin,
    pub simple_led_pin: SimpleLedPin,
    pub pio1: Pio1,
    pub pio1_sm0: Pio1SM0,
    pub alarm1: Alarm1,
}

/// Stack for core1
pub static CORE1_STACK: Stack<4096> = Stack::new();

/// Mutex for transferring BoardCore1 to core1
pub static CORE1_PERIPHERALS: Mutex<RefCell<Option<BoardCore1>>> = Mutex::new(RefCell::new(None));

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
            &mut watchdog,
        )
        .expect("ClocksManager init failed");

        let sio = hal::Sio::new(pac.SIO);

        let pins = hal::gpio::Pins::new(
            pac.IO_BANK0,
            pac.PADS_BANK0,
            sio.gpio_bank0,
            &mut pac.RESETS,
        );

        let mut timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);
        let alarm1 = timer.alarm_1().unwrap();

        // PIO0 for WS2812 RGB LED
        let (mut pio, sm0, _, _, _) = pac.PIO0.split(&mut pac.RESETS);

        // PIO1 for IR decoder
        let (pio1, pio1_sm0, _, _, _) = pac.PIO1.split(&mut pac.RESETS);

        let rgb_led_chain = Ws2812::new(
            pins.gpio6.into_function(),
            &mut pio,
            sm0,
            clocks.peripheral_clock.freq(),
            timer.count_down(),
        );

        let simple_led_pin = pins.gpio7.into_push_pull_output();
        let buzzer = pins.gpio20.into_push_pull_output();
        let ir_receiver_pin = pins.gpio0.into_pull_up_input();

        let multicore_peripherals = MulticorePeripherals {
            sio_fifo: sio.fifo,
            psm: pac.PSM,
            ppb: pac.PPB,
        };

        let board = Self {
            timer,
            watchdog,
            rgb_led_chain,
            buzzer,
            multicore_peripherals,
        };

        let board_core1 = BoardCore1 {
            ir_receiver_pin,
            simple_led_pin,
            pio1,
            pio1_sm0,
            alarm1,
        };

        (board, board_core1)
    }
}
