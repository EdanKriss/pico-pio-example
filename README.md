# Pico W Rust PIO Project

An exploratory Rust project for Raspberry Pi Pico W.
The PicoBricks dev board was used, but it is possible to recreate the setup with a few peripherals.

The Pico has one feature that is somewhat unique in its class: Programmable IO.
These effectively serve as 8 separate state machines that can operate on IO as it comes in with deterministic timing.

This project seeks to utilize PIO for input peripherals, such as the IR sensor.
Interrupts are registered to fire on incoming data, allowing core1 to sleep when data is not arriving.

Datasheets for Picobricks and supplied peripherals can be found here:
https://github.com/Robotistan/PicoBricks/blob/main/Documents/Datasheets

## Prerequisites

1. **Rust toolchain** - Rustup and Cargo were used in development

2. **ARM target** - Install with:
   ```bash
   rustup target add thumbv6m-none-eabi
   ```

3. **elf2uf2-rs** - Optional but recommended global for easy flashing:
   ```bash
   cargo install elf2uf2-rs
   ```

## Hardware Setup

**For PicoBricks:**
- This project is ready to flash to PicoBricks: the peripherals and GPIO pins match the PicoBricks.

**For custom setup:**
- Review the peripherals and GPIO pins used in `src/board.rs`

## Building

Build the project:
```bash
cargo build --release
```

The compiled binary will be in `target/thumbv6m-none-eabi/release/pico-project`

## Deploying to Pico W

### Method 1: Using elf2uf2-rs (Easiest)

1. Hold the BOOTSEL button on your Pico W while plugging it into USB
2. The Pico will appear as a USB mass storage device called "RPI-RP2"
3. Run:
   ```bash
   cargo run --release
   ```
   Or with elf2uf2-rs directly:
   ```bash
   elf2uf2-rs -d target/thumbv6m-none-eabi/release/pico-project
   ```

### Method 2: Manual UF2 Copy

1. Build and convert to UF2:
   ```bash
   cargo build --release
   elf2uf2-rs target/thumbv6m-none-eabi/release/pico-project pico-project.uf2
   ```

2. Hold BOOTSEL button while plugging in Pico W
3. Copy `pico-project.uf2` to the RPI-RP2 drive
4. The Pico will automatically reboot and run your program

### Method 3: Using probe-rs (Advanced - requires debug probe)

If you have a debug probe connected:
```bash
cargo install probe-rs --features cli
probe-rs run --chip RP2040 target/thumbv6m-none-eabi/release/pico-project
```

## What the Code Does

- Initializes the RP2040 hardware (clocks, GPIO, etc.)
- Configures GPIO 6 as an output pin
- Blinks the RGB LED on/off every 500ms in an infinite loop

## Next Steps

- Try changing the GPIO pin number (e.g., to GPIO 25 for onboard LED on regular Pico)
- Adjust the blink delay
- Add more LEDs on different GPIO pins
- Explore the PicoBricks sensors and actuators!

## Troubleshooting

**"No such device"** - Make sure you're holding BOOTSEL while plugging in the Pico

**Build errors** - Make sure you installed the ARM target:
```bash
rustup target add thumbv6m-none-eabi
```

**LED doesn't blink** - Check your wiring and resistor value
