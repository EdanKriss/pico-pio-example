# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Bare-metal Rust project for Raspberry Pi Pico W with PicoBricks. Uses RP2040 microcontroller (dual-core ARM Cortex-M0+). Currently blinks an RGB LED (WS2812) on GPIO 6 and a simple LED on GPIO 7.

## Build and Deploy Commands

```bash
# Build the project
cargo build --release

# Build and flash to Pico (requires elf2uf2-rs and Pico in BOOTSEL mode)
cargo run --release

# Manual UF2 conversion
elf2uf2-rs target/thumbv6m-none-eabi/release/pico-project pico-project.uf2
```

**Flashing**: Hold BOOTSEL button while plugging in Pico W. It mounts as "RPI-RP2" USB drive.

## Prerequisites

```bash
rustup target add thumbv6m-none-eabi
cargo install elf2uf2-rs
```

## Architecture

### Embedded Rust Stack
- **HAL** (`rp2040_hal`): Safe hardware abstraction layer
- **PAC** (`rp2040_pac`, via `hal::pac`): Raw register access
- **cortex-m-rt**: Runtime/entry point (`#[entry]`)
- **rp2040-boot2**: Second-stage bootloader (256 bytes in `.boot2` section)

### Key Files
- `src/main.rs`: Application code, `#![no_std]` and `#![no_main]`
- `memory.x`: Linker script defining BOOT2/FLASH/RAM memory regions
- `.cargo/config.toml`: Build target (`thumbv6m-none-eabi`), runner, rustflags

### RP2040 Initialization Pattern
Every program follows this sequence:
1. `Peripherals::take()` and `CorePeripherals::take()` - claim singletons
2. `Watchdog::new()` - needed for clock init
3. `init_clocks_and_plls()` - configure 12 MHz crystal to 125 MHz system clock
4. `Pins::new()` with SIO - set up GPIO
5. `Delay::new()` - timing provider

### Current Hardware Configuration
- GPIO 6: WS2812 RGB LED (via PIO peripheral)
- GPIO 7: Simple LED output
- GPIO 20: Buzzer for error indication
- Watchdog: 8 second timeout

## RP2040 Specifics

- **No FPU**: Cortex-M0+ has no floating-point unit. Use fixed-point math.
- **Crystal**: 12 MHz external oscillator
- **System clock**: 125 MHz after PLL configuration
- **PIO**: Used for WS2812 timing (smart LEDs need precise timing)

## Key Dependencies

- `rp2040-hal`: Hardware abstraction
- `ws2812-pio` + `smart-leds`: WS2812/NeoPixel LED control via PIO
- `fugit`: Time/duration types (`ExtU32` trait for `.secs()`, `.millis()`)
- `embedded-hal`: Standard hardware abstraction traits

## Rust 2024 Notes

This project uses Rust 2024 edition. The `#[unsafe(link_section = ".boot2")]` syntax is required (instead of plain `#[link_section]`).
