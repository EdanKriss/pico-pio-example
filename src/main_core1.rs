//! Core1 task orchestration
//!
//! Interrupt-driven IR reception with lock-free event signalling:
//!   - PIO1 raises PIO1_IRQ_0 whenever its RX FIFO becomes non-empty.
//!   - The ISR (`PIO1_IRQ_0`) drains the FIFO fully via `NecIrDecoder::poll`.
//!     Because FIFO-not-empty is a *level-triggered* condition, draining the
//!     FIFO causes the IRQ line to deassert — no masking dance required.
//!   - Decoded commands are pushed into an SPSC queue; the main loop dequeues
//!     them on wake. The queue is lock-free (atomic head/tail indices) so
//!     main's hot path stays critical-section-free.
//!   - A 50ms timer alarm wakes core1 for LED housekeeping (blink timeout).
//!
//! The ISR-side state (decoder + producer) lives in a
//! `critical_section::Mutex<RefCell<Option<_>>>` to handle the install window.
//! Once installed, only the ISR touches it, so the CS inside the ISR protects
//! against init races rather than real concurrency. Main never takes that CS.

use core::cell::RefCell;

use cortex_m::asm::wfi;
use cortex_m::peripheral::NVIC;
use critical_section::Mutex;
use embedded_hal::digital::OutputPin;
use fugit::MicrosDurationU32;
use heapless::spsc::{Consumer, Producer, Queue};
use rp2040_hal::multicore::Multicore;
use rp2040_hal::pac::{self, interrupt};
use rp2040_hal::timer::Alarm;

use crate::board::{BoardCore1, CORE1_PERIPHERALS, CORE1_STACK, MulticorePeripherals, SimpleLedPin};
use crate::ir_nec_pio::{IrCommand, NecIrDecoder};

/// Housekeeping interval: wakes core1 periodically to check whether the LED
/// blink window has expired. Does not need to be short — the IR path is
/// driven by PIO1_IRQ_0, not this alarm.
const HOUSEKEEPING_INTERVAL_US: u32 = 50_000;

/// Capacity of the ISR -> main SPSC event queue. Nominal traffic needs only
/// a couple of slots (main drains on every PIO wake), but 16 gives headroom
/// for misbehaving or multiple IR sources that burst frames faster than a
/// single remote's repeat rate. Overflow drops the newest command.
const EVENT_QUEUE_CAPACITY: usize = 16;

/// Backing storage for the SPSC queue. Split once at core1 init into a
/// `(Producer, Consumer)` pair; after that, the queue is accessed only via
/// those two handles — never through this static directly.
///
/// SAFETY: this `static mut` is referenced exactly once, via `Queue::split`
/// inside `main_core1`, before the PIO interrupt is enabled. From that point
/// on the Producer and Consumer are the sole access paths, and they are
/// lock-free between one producer and one consumer.
static mut EVENT_QUEUE: Queue<IrCommand, EVENT_QUEUE_CAPACITY> = Queue::new();

/// ISR-owned state: the decoder itself plus the producer end of the event
/// queue. Bundled so one critical section in the ISR grants access to both.
struct IrIsrState {
    decoder: NecIrDecoder,
    producer: Producer<'static, IrCommand, EVENT_QUEUE_CAPACITY>,
}

/// ISR state slot. Populated by `main_core1` before PIO1_IRQ_0 is unmasked.
static IR_ISR_STATE: Mutex<RefCell<Option<IrIsrState>>> = Mutex::new(RefCell::new(None));

/**
    Read the hardware timer (microseconds since boot).

    Returns valid data AFTER hal::Timer::new() in Board::init(), returns garbage data before

    Safety:
        - Data Race: TIMERAWL is a read-only register that both cores can read concurrently
        - Panic/Fault: peripheral registers in memory region 0x40000000 - 0x4FFFFFFF never faults on read

    Uses raw dereference rather than the HAL Timer struct because Timer is owned by core0.
    Raw register access is the standard approach in C/C++ embedded.

    Alternatives considered:
        - Move/Refer Timer to core1: Both cores need access, but Timer isn't Send/Sync
        - Wrap Timer in Mutex: Blocks cores on reads
        - Message passing via FIFO: Adds latency to reads
 */
#[inline(always)]
fn read_timer_us() -> u32 {
    unsafe { (*rp2040_hal::pac::TIMER::ptr()).timerawl().read().bits() }
}

/// LED blink state
struct LedBlinker {
    led: SimpleLedPin,
    on: bool,
    start_time: u32,
}

impl LedBlinker {
    const BLINK_DURATION_US: u32 = 100_000; // 100ms

    fn new(led: SimpleLedPin) -> Self {
        Self {
            led,
            on: false,
            start_time: 0,
        }
    }

    fn trigger(&mut self) {
        self.on = true;
        self.start_time = read_timer_us();
        let _ = self.led.set_high();
    }

    fn update(&mut self) {
        if self.on {
            let elapsed = read_timer_us().wrapping_sub(self.start_time);
            if elapsed >= Self::BLINK_DURATION_US {
                self.on = false;
                let _ = self.led.set_low();
            }
        }
    }
}

/// Core1 main loop. Sleeps in WFI; wakes on PIO1 FIFO events (ISR-driven)
/// and on the housekeeping timer alarm.
fn main_core1(
    BoardCore1 {
        ir_receiver_pin,
        simple_led_pin,
        pio1,
        pio1_sm0,
        alarm1,
    }: BoardCore1,
) -> ! {
    let ir = NecIrDecoder::new(ir_receiver_pin, pio1, pio1_sm0);
    let mut led = LedBlinker::new(simple_led_pin);
    let mut alarm = alarm1;

    // Split the SPSC queue once. The producer goes into ISR-owned state;
    // the consumer is a stack-local handle for the main loop.
    //
    // SAFETY: EVENT_QUEUE is accessed exactly once here, before the PIO
    // interrupt is enabled, so there is no concurrent access. The producer
    // and consumer returned are the only subsequent access paths to the
    // queue, and Queue::split is the documented way to obtain them.
    #[allow(static_mut_refs)]
    let (producer, mut consumer): (
        Producer<'static, IrCommand, EVENT_QUEUE_CAPACITY>,
        Consumer<'static, IrCommand, EVENT_QUEUE_CAPACITY>,
    ) = unsafe { EVENT_QUEUE.split() };

    // Install ISR-owned state, then enable the PIO interrupt.
    // Order matters: install first, then unmask — otherwise an early IRQ
    // could fire with IR_ISR_STATE still None.
    ir.enable_fifo_interrupt();
    critical_section::with(|cs| {
        IR_ISR_STATE.borrow(cs).replace(Some(IrIsrState {
            decoder: ir,
            producer,
        }));
    });
    unsafe { NVIC::unmask(pac::Interrupt::PIO1_IRQ_0); }

    // Housekeeping alarm for LED blink timeout.
    alarm.enable_interrupt();
    unsafe { NVIC::unmask(pac::Interrupt::TIMER_IRQ_1); }
    let _ = alarm.schedule(MicrosDurationU32::micros(HOUSEKEEPING_INTERVAL_US));

    loop {
        wfi();

        // Lock-free dequeue — no critical section on main's hot path.
        while let Some(_cmd) = consumer.dequeue() {
            led.trigger();
        }
        led.update();

        let _ = alarm.schedule(MicrosDurationU32::micros(HOUSEKEEPING_INTERVAL_US));
    }
}

/// PIO1 FIFO-not-empty interrupt handler.
///
/// Drains the FIFO completely by calling `poll()` until it returns `None`.
/// Because FIFO-not-empty is level-triggered on the RP2040 PIO, draining the
/// FIFO causes the peripheral IRQ line to deassert automatically — no NVIC
/// masking needed, no race with the main loop. Each decoded command is
/// pushed into the SPSC event queue; overflow drops the newest command
/// (acceptable — the main loop will have drained by the next NEC frame).
#[interrupt]
fn PIO1_IRQ_0() {
    critical_section::with(|cs| {
        if let Some(state) = IR_ISR_STATE.borrow(cs).borrow_mut().as_mut() {
            while let Some(cmd) = state.decoder.poll() {
                let _ = state.producer.enqueue(cmd);
            }
        }
    });
}

/// Timer interrupt handler - clears interrupt so WFI returns
#[interrupt]
fn TIMER_IRQ_1() {
    // Clear at hardware level to prevent immediate re-fire.
    // Equivalent to Alarm1.clear_interrupt() without needing the alarm object to be scoped
    // to the interrupt, which would require static ownership and a critical_section on every 
    // interrupt occurrance.
    unsafe {
        (*pac::TIMER::ptr()).intr().write(|w| w.alarm_1().clear_bit_by_one());
    }
}

/// Spawn core1 with the given peripherals
pub fn spawn(
    multicore_peripherals: &mut MulticorePeripherals,
    board_core_1: BoardCore1,
) {
    // Store peripherals for core1 to take
    critical_section::with(|cs| {
        CORE1_PERIPHERALS.borrow(cs).replace(Some(board_core_1));
    });

    // Configure core1
    let mut mc = Multicore::new(
        &mut multicore_peripherals.psm,
        &mut multicore_peripherals.ppb,
        &mut multicore_peripherals.sio_fifo,
    );
    let core1 = &mut mc.cores()[1];
    let stack = CORE1_STACK.take().expect("CORE1_STACK already taken");

    // Spawn core1
    core1
    .spawn(stack, || {
        // Take ownership of peripherals
        let p = critical_section::with(|cs| {
            CORE1_PERIPHERALS.borrow(cs).take().expect("Core1 peripherals not set")
        });
        main_core1(p)
    })
    .expect("core1 spawn failed");
}
