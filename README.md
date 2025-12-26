# Pico W Rust Project

A barebones Rust project for Raspberry Pi Pico W with PicoBricks. This blinks the RGB LED on GPIO 6.

## Prerequisites

1. **Rust toolchain** - You already have this if you created the project
2. **ARM target** - Install with:
   ```bash
   rustup target add thumbv6m-none-eabi
   ```

3. **elf2uf2-rs** (optional but recommended) - For easy flashing:
   ```bash
   cargo install elf2uf2-rs
   ```

## Hardware Setup

**For PicoBricks:**
- The RGB LED is already connected to GPIO 6 (GP6 terminal)
- No additional wiring needed - just flash and run!

**For custom setup:**
- Connect an LED between GPIO 6 and GND with a 220-330Ω resistor

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
