use core::cell::RefCell;

use fugit::RateExtU32;
use heapless::spsc::{Consumer, Producer, Queue};
use rp2040_hal as hal;
use hal::gpio::{FunctionI2C, FunctionPio0, FunctionPio1, FunctionSio, Pin, PullDown, PullUp, SioInput, SioOutput};
use hal::gpio::bank0::{Gpio0, Gpio4, Gpio5, Gpio6, Gpio7, Gpio20};
use hal::multicore::Stack;
use hal::pio::PIOExt;
use hal::sio::SioFifo;
use hal::Clock;
use critical_section::Mutex;
use ssd1306::{
    I2CDisplayInterface, Ssd1306,
    mode::{BufferedGraphicsMode, DisplayConfig},
    prelude::{DisplayRotation, I2CInterface},
    size::DisplaySize128x64,
};
use ws2812_pio::Ws2812;

use crate::ir_nec_pio::IrCommand;

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

/// I2C0 bus to the PicoBricks peripherals. SDA=GPIO4, SCL=GPIO5.
pub type SdaPin = Pin<Gpio4, FunctionI2C, PullUp>;
pub type SclPin = Pin<Gpio5, FunctionI2C, PullUp>;
pub type I2cBus = hal::I2C<hal::pac::I2C0, (SdaPin, SclPin)>;

/// SSD1306 128x64 OLED in buffered graphics mode.
pub type OledDisplay = Ssd1306<
    I2CInterface<I2cBus>,
    DisplaySize128x64,
    BufferedGraphicsMode<DisplaySize128x64>,
>;

/// Capacity of the core1 -> core0 IR forwarding queue. Matches the ISR -> core1
/// queue; the OLED only cares about the latest anyway, overflow drops newest.
pub const CORE0_IR_QUEUE_CAPACITY: usize = 16;

pub type Core0IrProducer = Producer<'static, IrCommand, CORE0_IR_QUEUE_CAPACITY>;
pub type Core0IrConsumer = Consumer<'static, IrCommand, CORE0_IR_QUEUE_CAPACITY>;

/// Backing storage for the core1 -> core0 forwarding queue. Split exactly once
/// inside `Board::init()` before core1 is spawned; after that the Producer
/// (moved into BoardCore1) and Consumer (returned to main) are the only
/// access paths.
static mut CORE0_IR_QUEUE: Queue<IrCommand, CORE0_IR_QUEUE_CAPACITY> = Queue::new();

/// Peripherals owned by core0
pub struct Board {
    pub timer: hal::Timer,
    pub watchdog: hal::Watchdog,
    pub rgb_led_chain: RgbLedChain,
    pub buzzer: BuzzerPin,
    pub oled: OledDisplay,
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
    pub ir_command_producer: Core0IrProducer,
}

/// Stack for core1
pub static CORE1_STACK: Stack<4096> = Stack::new();

/// Mutex for transferring BoardCore1 to core1
pub static CORE1_PERIPHERALS: Mutex<RefCell<Option<BoardCore1>>> = Mutex::new(RefCell::new(None));

/**
    Read the hardware timer (microseconds since boot).

    Returns valid data AFTER hal::Timer::new() in Board::init()

    Safety:
        - Data Race: TIMERAWL is a read-only register that both cores can read concurrently
        - Panic/Fault: peripheral registers in memory region 0x40000000 - 0x4FFFFFFF never faults on read

    Uses raw register access because the HAL Timer struct is owned by core0.
    Raw register access is the standard approach in C/C++ embedded.

    Alternatives considered:
        - Move/Refer Timer to core1: Both cores need access, but Timer isn't Send/Sync
        - Wrap Timer in Mutex: Blocks cores on reads
        - Message passing via FIFO: Adds latency to reads
 */
#[inline(always)]
pub fn read_timer_us() -> u32 {
    unsafe { (*hal::pac::TIMER::ptr()).timerawl().read().bits() }
}

impl Board {
    pub fn init() -> (Self, BoardCore1, Core0IrConsumer) {
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

        // I2C0 on GPIO4 (SDA) / GPIO5 (SCL) at 400kHz (SSD1306 fast mode).
        // The PicoBricks carrier has external pull-ups; the internal PullUp
        // is belt-and-braces.
        let sda: SdaPin = pins.gpio4.reconfigure();
        let scl: SclPin = pins.gpio5.reconfigure();
        let i2c = hal::I2C::i2c0(
            pac.I2C0,
            sda,
            scl,
            400.kHz(),
            &mut pac.RESETS,
            &clocks.system_clock,
        );

        let interface = I2CDisplayInterface::new(i2c);
        let mut oled = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();
        // Silent failure if the OLED is absent/unresponsive - the rest of the
        // board should still function. Any later flush() will error-out too.
        let _ = oled.init();
        // SSD1306 GDDRAM is independently powered. Flush framebuffer to prevent
        // a checkered screen on cold boot, and to clear the previous session's pixels
        // after an MCU reset.
        let _ = oled.flush();

        // SAFETY: Queue::split is called exactly once here, before core1 is
        // spawned, so there is no concurrent access. The Producer (moved into
        // BoardCore1) and the Consumer (returned to main) are the only
        // subsequent access paths.
        #[allow(static_mut_refs)]
        let (ir_command_producer, ir_command_consumer) = unsafe { CORE0_IR_QUEUE.split() };

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
            oled,
            multicore_peripherals,
        };

        let board_core1 = BoardCore1 {
            ir_receiver_pin,
            simple_led_pin,
            pio1,
            pio1_sm0,
            alarm1,
            ir_command_producer,
        };

        (board, board_core1, ir_command_consumer)
    }
}
