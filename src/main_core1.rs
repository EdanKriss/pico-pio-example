//! Core1 task orchestration
//!
//! Interrupt-driven IR reception:
//!   - PIO1 raises PIO1_IRQ_0 whenever its RX FIFO becomes non-empty.
//!   - The ISR (`PIO1_IRQ_0`) drains the FIFO fully via `NecIrDecoder::poll`.
//!     Because FIFO-not-empty is a *level-triggered* condition, draining the
//!     FIFO causes the IRQ line to deassert — no masking dance required.
//!   - Decoded commands set `IR_EVENT`; the main loop consumes it on wake.
//!   - A 50ms timer alarm wakes core1 for LED housekeeping (blink timeout).
//!
//! The decoder lives in a `critical_section::Mutex<RefCell<Option<_>>>` so the
//! ISR can borrow it exclusively. Core1 is the only accessor after init, so
//! the critical section serves correctness against preemption, not cross-core.

use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, Ordering};

use cortex_m::asm::wfi;
use cortex_m::peripheral::NVIC;
use critical_section::Mutex;
use embedded_hal::digital::OutputPin;
use fugit::MicrosDurationU32;
use rp2040_hal::multicore::Multicore;
use rp2040_hal::pac::{self, interrupt};
use rp2040_hal::timer::Alarm;

use crate::board::{BoardCore1, CORE1_PERIPHERALS, CORE1_STACK, MulticorePeripherals, SimpleLedPin};
use crate::ir_nec_pio::NecIrDecoder;

/// Housekeeping interval: wakes core1 periodically to check whether the LED
/// blink window has expired. Does not need to be short — the IR path is
/// driven by PIO1_IRQ_0, not this alarm.
const HOUSEKEEPING_INTERVAL_US: u32 = 50_000;

/// Decoder owned by the PIO1_IRQ_0 ISR after init. Core1-only.
static IR_DECODER: Mutex<RefCell<Option<NecIrDecoder>>> = Mutex::new(RefCell::new(None));

/// Set by the ISR when a complete NEC command has been decoded.
/// Main loop consumes it via `swap(false)` on wake.
static IR_EVENT: AtomicBool = AtomicBool::new(false);

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

    // Hand the decoder to the ISR, then enable the PIO interrupt.
    // Order matters: install first, then unmask — otherwise an early IRQ
    // could fire with IR_DECODER still None.
    ir.enable_fifo_interrupt();
    critical_section::with(|cs| {
        IR_DECODER.borrow(cs).replace(Some(ir));
    });
    unsafe { NVIC::unmask(pac::Interrupt::PIO1_IRQ_0); }

    // Housekeeping alarm for LED blink timeout.
    alarm.enable_interrupt();
    unsafe { NVIC::unmask(pac::Interrupt::TIMER_IRQ_1); }
    let _ = alarm.schedule(MicrosDurationU32::micros(HOUSEKEEPING_INTERVAL_US));

    loop {
        wfi();

        // Cortex-M0+ has no HW atomic RMW; emulate swap via a critical section.
        let event = critical_section::with(|_| {
            let v = IR_EVENT.load(Ordering::Relaxed);
            IR_EVENT.store(false, Ordering::Relaxed);
            v
        });
        if event {
            led.trigger();
        }
        led.update();

        alarm.clear_interrupt();
        let _ = alarm.schedule(MicrosDurationU32::micros(HOUSEKEEPING_INTERVAL_US));
    }
}

/// PIO1 FIFO-not-empty interrupt handler.
///
/// Drains the FIFO completely by calling `poll()` until it returns `None`.
/// Because FIFO-not-empty is level-triggered on the RP2040 PIO, draining the
/// FIFO causes the peripheral IRQ line to deassert automatically — no NVIC
/// masking needed, no race with the main loop.
#[interrupt]
fn PIO1_IRQ_0() {
    critical_section::with(|cs| {
        if let Some(dec) = IR_DECODER.borrow(cs).borrow_mut().as_mut() {
            while dec.poll().is_some() {
                IR_EVENT.store(true, Ordering::Relaxed);
            }
        }
    });
}

/// Timer interrupt handler - clears interrupt so WFI returns
#[interrupt]
fn TIMER_IRQ_1() {
    // Clear at hardware level to prevent immediate re-fire
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
