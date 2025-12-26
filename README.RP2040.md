# RP2040 Primer & Cheat Sheet

A beginner-friendly guide to programming the RP2040 (Raspberry Pi Pico) in Rust.

---

## Table of Contents

1. [What is the RP2040?](#what-is-the-rp2040)
2. [The Embedded Rust Stack](#the-embedded-rust-stack)
3. [Two Types of Peripherals](#two-types-of-peripherals)
4. [RP2040 Peripheral Reference](#rp2040-peripheral-reference)
5. [Getting Started Pattern](#getting-started-pattern)
6. [Common Recipes](#common-recipes)
7. [Pin Reference](#pin-reference)
8. [Clock System](#clock-system)
9. [Glossary](#glossary)
10. [Essential Crates](#essential-crates)
11. [Quick Reference Card](#quick-reference-card)

---

## What is the RP2040?

The RP2040 is a microcontroller chip designed by Raspberry Pi. Key specs:

| Feature | Value |
|---------|-------|
| CPU | Dual-core ARM Cortex-M0+ @ 133 MHz |
| RAM | 264 KB SRAM |
| Flash | External (2 MB on Pico board) |
| GPIO | 30 pins (directly usable: 26) |
| ADC | 12-bit, 5 channels (4 external + temp sensor) |
| USB | USB 1.1 Host/Device |
| Special | 2x PIO (Programmable I/O) blocks |

**No FPU** - The Cortex-M0+ has no floating-point unit. Float math is done in software (slow). Use fixed-point math when possible.

---

## The Embedded Rust Stack

| Layer | Crate | Description |
|-------|-------|-------------|
| Application | *your code* | Business logic |
| HAL (Hardware Abstraction Layer) | `rp2040_hal` | Safe, ergonomic API |
| PAC (Peripheral Access Crate) | `rp2040_pac` | Raw register access |
| Hardware | RP2040 | Physical chip |

- **PAC**: Auto-generated from SVD files. Provides raw, unsafe register access.
- **HAL**: Wraps the PAC with safe, idiomatic Rust APIs.
- You typically only interact with the HAL. The PAC is re-exported via `hal::pac`.

---

## Two Types of Peripherals

When you call `take()`, you get two separate singleton structs:

### CorePeripherals (ARM Cortex-M standard)

These are part of the ARM Cortex-M0+ core itself. Same on ALL Cortex-M chips.

```rust
let core = hal::pac::CorePeripherals::take().unwrap();
```

| Peripheral | Description |
|------------|-------------|
| NVIC | Nested Vectored Interrupt Controller |
| SCB | System Control Block |
| SysTick | System timer (used for delays) |
| DWT | Data Watchpoint and Trace |
| MPU | Memory Protection Unit (optional on M0+) |
| FPU | Floating Point Unit (NOT present on M0+) |

### Peripherals (RP2040-specific)

These are unique to the RP2040 chip.

```rust
let mut pac = hal::pac::Peripherals::take().unwrap();
```

See full list in [RP2040 Peripheral Reference](#rp2040-peripheral-reference).

---

## RP2040 Peripheral Reference

### Clocks & Oscillators

| Peripheral | Description |
|------------|-------------|
| XOSC | External Crystal Oscillator (12 MHz on Pico) |
| ROSC | Ring Oscillator (internal, ~6 MHz, imprecise) |
| CLOCKS | Clock generation and distribution |
| PLL_SYS | System PLL (generates up to 133 MHz) |
| PLL_USB | USB PLL (generates 48 MHz for USB) |

### GPIO & I/O

| Peripheral | Description |
|------------|-------------|
| IO_BANK0 | GPIO control (pin functions, interrupts) |
| IO_QSPI | QSPI flash pin control |
| PADS_BANK0 | GPIO pad control (drive strength, pull-ups/downs, slew rate) |
| PADS_QSPI | QSPI pad control |
| SIO | Single-cycle I/O (fast GPIO, spinlocks, hardware divider) |

### Communication

| Peripheral | Description |
|------------|-------------|
| UART0, UART1 | Serial ports (TX/RX) |
| SPI0, SPI1 | SPI controllers |
| I2C0, I2C1 | I2C controllers |
| USB | USB 1.1 controller |
| USBCTRL_REGS | USB control registers |
| USBCTRL_DPRAM | USB data RAM |

### Timers & Watchdog

| Peripheral | Description |
|------------|-------------|
| TIMER | 64-bit microsecond timer with 4 alarms |
| WATCHDOG | Watchdog timer (resets chip if not fed) |
| RTC | Real-time clock |

### Programmable I/O

| Peripheral | Description |
|------------|-------------|
| PIO0, PIO1 | Programmable I/O blocks (4 state machines each) |

PIO is the RP2040's superpower. It can implement custom protocols (WS2812, VGA, etc.) with precise timing that the CPU can't achieve.

### Analog

| Peripheral | Description |
|------------|-------------|
| ADC | 12-bit ADC with 5 channels |

Channels: GPIO26 (ADC0), GPIO27 (ADC1), GPIO28 (ADC2), GPIO29 (ADC3), Internal temp sensor (ADC4)

### DMA & Memory

| Peripheral | Description |
|------------|-------------|
| DMA | 12-channel DMA controller |
| XIP_CTRL | Execute-in-place controller (runs code from flash) |
| XIP_SSI | Flash SPI interface |

### System

| Peripheral | Description |
|------------|-------------|
| RESETS | Peripheral reset control (must release reset before using a peripheral) |
| PSM | Power-on state machine |
| SYSCFG | System configuration |
| SYSINFO | Chip information (ID, revision) |
| VREG_AND_CHIP_RESET | Voltage regulator & reset control |
| TBMAN | Testbench manager |

### Debug

| Peripheral | Description |
|------------|-------------|
| PPB | Private Peripheral Bus (debug access) |

### Multicore

| Feature | Description |
|---------|-------------|
| SIO FIFO | Inter-core communication (8 words each direction) |
| Spinlocks | 32 hardware spinlocks for synchronization |

---

## Getting Started Pattern

Every RP2040 Rust program follows this pattern:

```rust
#![no_std]  // No standard library (we're on bare metal)
#![no_main] // No main() - we define our own entry point

use rp2040_hal as hal;
use hal::pac;
use cortex_m_rt::entry;

#[entry]
fn main() -> ! {
    // 1. Take the peripherals (singletons - can only do this once!)
    let mut pac = pac::Peripherals::take().unwrap();
    let core = pac::CorePeripherals::take().unwrap();

    // 2. Set up the watchdog (needed for clock init)
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);

    // 3. Configure clocks
    let clocks = hal::clocks::init_clocks_and_plls(
        12_000_000u32,        // Crystal frequency
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();

    // 4. Set up GPIO
    let sio = hal::Sio::new(pac.SIO);
    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // 5. Set up delay provider
    let mut delay = cortex_m::delay::Delay::new(
        core.SYST,
        clocks.system_clock.freq().to_Hz(),
    );

    // 6. Your code here!
    loop {
        // Main loop (must never return, hence -> !)
    }
}
```

---

## Common Recipes

### Blink an LED

```rust
use embedded_hal::digital::OutputPin;

let mut led = pins.gpio25.into_push_pull_output(); // On-board LED on Pico

loop {
    led.set_high().unwrap();
    delay.delay_ms(500);
    led.set_low().unwrap();
    delay.delay_ms(500);
}
```

### Read a Button

```rust
use embedded_hal::digital::InputPin;

let button = pins.gpio15.into_pull_up_input();

loop {
    if button.is_low().unwrap() {
        // Button pressed (pull-up means LOW = pressed)
    }
}
```

### UART Serial Output

```rust
use hal::uart::{UartConfig, DataBits, StopBits};
use fugit::RateExtU32;

let uart_pins = (
    pins.gpio0.into_function(),  // TX
    pins.gpio1.into_function(),  // RX
);

let mut uart = hal::uart::UartPeripheral::new(
    pac.UART0,
    uart_pins,
    &mut pac.RESETS,
)
.enable(
    UartConfig::new(115200.Hz(), DataBits::Eight, None, StopBits::One),
    clocks.peripheral_clock.freq(),
)
.unwrap();

use core::fmt::Write;
writeln!(uart, "Hello, world!").unwrap();
```

### Read ADC

```rust
use hal::adc::Adc;
use embedded_hal::adc::OneShot;

let mut adc = Adc::new(pac.ADC, &mut pac.RESETS);
let mut adc_pin = hal::adc::AdcPin::new(pins.gpio26).unwrap();

loop {
    let value: u16 = adc.read(&mut adc_pin).unwrap();
    // value is 0-4095 (12-bit)
}
```

### PWM Output

```rust
use hal::pwm::Slices;

let pwm_slices = Slices::new(pac.PWM, &mut pac.RESETS);
let mut pwm = pwm_slices.pwm4;
pwm.set_ph_correct();
pwm.enable();

let channel = &mut pwm.channel_a;
channel.output_to(pins.gpio8);
channel.set_duty(32768); // 50% duty cycle (0-65535)
```

### Use the Watchdog

```rust
use fugit::ExtU32;

let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);
watchdog.start(1_000_000.micros()); // 1 second timeout

loop {
    // Do work...
    watchdog.feed(); // Reset the timer (must call before timeout!)
}
```

---

## Pin Reference

### Raspberry Pi Pico Pinout

```
          ┌─────────────────┐
     GP0 ─┤ 1            40 ├─ VBUS
     GP1 ─┤ 2            39 ├─ VSYS
     GND ─┤ 3            38 ├─ GND
     GP2 ─┤ 4            37 ├─ 3V3_EN
     GP3 ─┤ 5            36 ├─ 3V3
     GP4 ─┤ 6            35 ├─ ADC_VREF
     GP5 ─┤ 7            34 ├─ GP28 (ADC2)
     GND ─┤ 8            33 ├─ GND
     GP6 ─┤ 9            32 ├─ GP27 (ADC1)
     GP7 ─┤ 10           31 ├─ GP26 (ADC0)
     GP8 ─┤ 11           30 ├─ RUN
     GP9 ─┤ 12           29 ├─ GP22
     GND ─┤ 13           28 ├─ GND
    GP10 ─┤ 14           27 ├─ GP21
    GP11 ─┤ 15           26 ├─ GP20
    GP12 ─┤ 16           25 ├─ GP19
    GP13 ─┤ 17           24 ├─ GP18
     GND ─┤ 18           23 ├─ GND
    GP14 ─┤ 19           22 ├─ GP17
    GP15 ─┤ 20           21 ├─ GP16
          └─────────────────┘

    On-board LED: GP25 (directly on Pico)
```

### Pin Functions

Each GPIO can be assigned different functions:

| Function | Description |
|----------|-------------|
| SIO | Software-controlled GPIO (default) |
| SPI | SPI peripheral |
| UART | UART peripheral |
| I2C | I2C peripheral |
| PWM | PWM output |
| PIO0, PIO1 | Programmable I/O |
| USB | USB signals |

Use `.into_function()` to set the pin function:

```rust
let tx_pin = pins.gpio0.into_function::<hal::gpio::FunctionUart>();
// Or let type inference figure it out:
let tx_pin = pins.gpio0.into_function();
```

---

## Clock System

The RP2040 has a flexible clock system:

```
XOSC (12 MHz) ──┬──> PLL_SYS ──> clk_sys (up to 133 MHz)
                │
                └──> PLL_USB ──> clk_usb (48 MHz)

ROSC (~6 MHz) ──> Backup clock (imprecise)
```

After `init_clocks_and_plls()`, you have access to:

| Clock | Typical Frequency | Use |
|-------|-------------------|-----|
| `system_clock` | 125 MHz | CPU, most peripherals |
| `peripheral_clock` | 125 MHz | UART, SPI, I2C |
| `usb_clock` | 48 MHz | USB |
| `adc_clock` | 48 MHz | ADC |
| `rtc_clock` | 46875 Hz | RTC |

```rust
use hal::Clock; // Import the trait

let sys_freq = clocks.system_clock.freq().to_Hz(); // e.g., 125_000_000
```

---

## Glossary

| Term | Definition |
|------|------------|
| **PAC** | Peripheral Access Crate - raw register access |
| **HAL** | Hardware Abstraction Layer - safe Rust API |
| **SVD** | System View Description - XML file describing chip registers |
| **GPIO** | General Purpose Input/Output |
| **PIO** | Programmable I/O - RP2040's custom state machines |
| **SIO** | Single-cycle I/O - fast GPIO access |
| **NVIC** | Nested Vectored Interrupt Controller |
| **PLL** | Phase-Locked Loop - multiplies clock frequency |
| **XOSC** | Crystal Oscillator |
| **ROSC** | Ring Oscillator |
| **DMA** | Direct Memory Access |
| **XIP** | Execute In Place - run code directly from flash |
| **ADC** | Analog-to-Digital Converter |
| **PWM** | Pulse Width Modulation |
| **Watchdog** | Timer that resets chip if not periodically "fed" |
| **Singleton** | Pattern ensuring only one instance exists |
| **`#![no_std]`** | No standard library (bare metal) |
| **`-> !`** | Function never returns (infinite loop) |

---

## Essential Crates

| Crate | Purpose |
|-------|---------|
| `rp2040-hal` | Hardware Abstraction Layer |
| `rp2040-boot2` | Second-stage bootloader |
| `cortex-m` | Low-level Cortex-M access |
| `cortex-m-rt` | Runtime (entry point, interrupts) |
| `embedded-hal` | Hardware abstraction traits |
| `fugit` | Time/frequency types |
| `panic-halt` | Simple panic handler (halts) |
| `panic-probe` | Panic handler for debugging |
| `defmt` | Efficient logging framework |

---

## Quick Reference Card

```
┌────────────────────────────────────────────────────────────┐
│                    RP2040 QUICK REFERENCE                  │
├────────────────────────────────────────────────────────────┤
│ CPU: Dual Cortex-M0+ @ 133MHz    RAM: 264KB    Flash: 2MB  │
├────────────────────────────────────────────────────────────┤
│ PERIPHERALS INIT ORDER:                                    │
│   1. Peripherals::take()      5. Pins::new()               │
│   2. CorePeripherals::take()  6. Delay::new()              │
│   3. Watchdog::new()          7. Your peripherals          │
│   4. init_clocks_and_plls()                                │
├────────────────────────────────────────────────────────────┤
│ GPIO MODES:                                                │
│   .into_push_pull_output()    .into_pull_up_input()        │
│   .into_pull_down_input()     .into_floating_input()       │
│   .into_function()            .into_readable_output()      │
├────────────────────────────────────────────────────────────┤
│ COMMON TRAITS (use embedded_hal::digital::*):              │
│   OutputPin: set_high(), set_low(), toggle()               │
│   InputPin:  is_high(), is_low()                           │
├────────────────────────────────────────────────────────────┤
│ DELAY:                                                     │
│   delay.delay_ms(500);        delay.delay_us(100);         │
├────────────────────────────────────────────────────────────┤
│ BUILD & FLASH:                                             │
│   cargo build --release                                    │
│   elf2uf2-rs target/thumbv6m-none-eabi/release/app         │
│   # Or hold BOOTSEL, plug in, drag .uf2 to RPI-RP2         │
└────────────────────────────────────────────────────────────┘
```
