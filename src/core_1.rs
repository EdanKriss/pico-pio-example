use core::cell::RefCell;

use rp2040_hal::multicore::Multicore;
use critical_section::Mutex;
use embedded_hal::digital::{InputPin, OutputPin};

use crate::board::{Board, BoardCore1, CORE1_STACK, IrReceiverPin, SimpleLedPin};
use crate::ir_remote::{IrRemote, SAMPLE_PERIOD_US};

/// IR receiver pin, transferred from core0 to core1 via critical section
static IR_PIN: Mutex<RefCell<Option<IrReceiverPin>>> = Mutex::new(RefCell::new(None));

/// Simple LED pin, transferred from core0 to core1 via critical section
static LED_PIN: Mutex<RefCell<Option<SimpleLedPin>>> = Mutex::new(RefCell::new(None));

/**
    Read the hardware timer (microseconds since boot).

    Returns valid data AFTER hal::Timer::new() in Board::init(), returns garbage data before

    Safety:
        - Data Race: TIMERAWL is a read-only register that both cores can read concurrently
        - Panic/Fault: peripheral registers in memory region 0x40000000 - 0x4FFFFFFF never faults on read

    Uses raw PAC dereference rather than the HAL Timer struct because Timer
    is owned by core0 and isn't Sync. Raw register access is the standard approach in C/C++ embedded and is
    appropriate here.

    Alternatives considered:
        - Pass Timer to core1: Not possible, Timer isn't Send/Sync
        - Wrap Timer in Mutex: Would block cores on every read
        - Message passing via FIFO: Adds latency for simple reads
 */
#[inline(always)]
fn read_timer_us() -> u32 {
    unsafe { (*rp2040_hal::pac::TIMER::ptr()).timerawl().read().bits() }
}

/// Delay for specified microseconds using hardware timer
#[inline(always)]
fn delay_us_core1(us: u32) {
    let start = read_timer_us();
    while read_timer_us().wrapping_sub(start) < us {}
}

/// Core1 entry point: polls IR receiver and blinks LED on decoded commands
fn core1_task() {
    // Take ownership of pins from the statics
    let mut ir_pin = critical_section::with(|cs| {
        IR_PIN.borrow(cs).take().expect("IR pin not initialized")
    });
    let mut led_pin = critical_section::with(|cs| {
        LED_PIN.borrow(cs).take().expect("LED pin not initialized")
    });

    let mut ir_remote = IrRemote::new();

    // Non-blocking LED blink state
    let mut led_on = false;
    let mut led_start_time: u32 = 0;
    const BLINK_DURATION_US: u32 = 100_000; // 100ms blink

    loop {
        let now = read_timer_us();

        // Handle non-blocking LED blink (wraparound-safe)
        if led_on {
            let elapsed = now.wrapping_sub(led_start_time);
            if elapsed >= BLINK_DURATION_US {
                led_on = false;
            }
        }

        if led_on {
            let _ = led_pin.set_high();
        } else {
            let _ = led_pin.set_low();
        }

        // Poll IR receiver
        let ir_signal = ir_pin.is_low().unwrap_or(false);

        if let Some(_cmd) = ir_remote.poll_raw(ir_signal) {
            // Start a new blink
            led_on = true;
            led_start_time = now;
        }

        delay_us_core1(SAMPLE_PERIOD_US);
    }
}

pub fn start_core_1(
    board: &mut Board,
    peripherals_core1: BoardCore1,
) {
    // Store pins in statics for core1 to take ownership
    critical_section::with(|cs| {
        IR_PIN.borrow(cs).replace(Some(peripherals_core1.ir_receiver));
        LED_PIN.borrow(cs).replace(Some(peripherals_core1.simple_led));
    });

    // Spawn core1 to handle IR receiver and LED
    let mut mc = Multicore::new(
        &mut board.mc.psm,
        &mut board.mc.ppb,
        &mut board.mc.sio_fifo,
    );
    let core1 = &mut mc.cores()[1];
    let stack = CORE1_STACK.take().unwrap();
    core1.spawn(stack, core1_task).unwrap();
}
