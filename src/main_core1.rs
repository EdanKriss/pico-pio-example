//! Core1 task orchestration
//!
//! This module owns the core1 event loop and coordinates core1 peripherals.
//! Uses interrupt-driven sleep (WFI) with timer alarm to wake periodically,
//! avoiding wasted CPU cycles from busy-polling.

use cortex_m::asm::wfi;
use cortex_m::peripheral::NVIC;
use embedded_hal::digital::OutputPin;
use fugit::MicrosDurationU32;
use rp2040_hal::multicore::Multicore;
use rp2040_hal::pac::{self, interrupt};
use rp2040_hal::timer::Alarm;

use crate::board::{BoardCore1, CORE1_PERIPHERALS, CORE1_STACK, MulticorePeripherals, SimpleLedPin};
use crate::ir_nec_pio::NecIrDecoder;

/// Housekeeping interval for LED timeout checks (50ms)
const HOUSEKEEPING_INTERVAL_US: u32 = 50_000;

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

/// Core1 main loop with interrupt-driven wake
fn main_core1(
    BoardCore1 {
        ir_receiver_pin,
        simple_led_pin,
        pio1,
        pio1_sm0,
        alarm1,
    }: BoardCore1,
) -> ! {
    let mut ir = NecIrDecoder::new(ir_receiver_pin, pio1, pio1_sm0);
    let mut led = LedBlinker::new(simple_led_pin);
    let mut alarm = alarm1;

    // Configure PIO FIFO interrupt for instant IR wake
    ir.enable_fifo_interrupt();
    unsafe { NVIC::unmask(pac::Interrupt::PIO1_IRQ_0); }

    // Configure timer alarm for LED housekeeping (50ms)
    alarm.enable_interrupt();
    unsafe { NVIC::unmask(pac::Interrupt::TIMER_IRQ_1); }
    let _ = alarm.schedule(MicrosDurationU32::micros(HOUSEKEEPING_INTERVAL_US));

    loop {
        wfi(); // Sleep until PIO data OR timer housekeeping

        // Check for IR data (PIO interrupt woke us)
        // TODO: Replace has_data() with poll() once decoder bug is fixed
        if ir.has_data() {
            led.trigger();
            ir.reset();
            // TODO: Send decoded command to core0 via FIFO
        }
        // Re-enable PIO interrupt (was masked in ISR)
        unsafe { NVIC::unmask(pac::Interrupt::PIO1_IRQ_0); }

        // Update LED state (timer interrupt woke us for housekeeping)
        led.update();

        // Reschedule timer alarm
        alarm.clear_interrupt();
        let _ = alarm.schedule(MicrosDurationU32::micros(HOUSEKEEPING_INTERVAL_US));
    }
}

/// PIO FIFO interrupt handler - masks interrupt so WFI returns
/// (FIFO not-empty is level-triggered, so we mask instead of clear)
#[interrupt]
fn PIO1_IRQ_0() {
    NVIC::mask(pac::Interrupt::PIO1_IRQ_0);
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
