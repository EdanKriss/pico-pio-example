//! SSD1306 OLED rendering for decoded IR commands.
//!
//! Thin layer over the shared `OledDisplay` in `board.rs`: formats an
//! `IrCommand` into two lines of text (hex bytes + button name) and flushes
//! to the display. Errors are silently discarded - if the OLED is missing or
//! unresponsive the rest of the firmware should keep running.

use core::fmt::Write;

use embedded_graphics::{
    mono_font::{ascii::FONT_6X13, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Baseline, Text},
};
use heapless::String;

use crate::board::OledDisplay;
use crate::ir_nec_pio::{buttons, IrCommand};

/// Render the latest IR command to the OLED. Clears the previous frame first
/// so the screen always shows the most recent command, not a stack of them.
pub fn show_command(display: &mut OledDisplay, cmd: IrCommand) {
    display.clear_buffer();

    let style = MonoTextStyle::new(&FONT_6X13, BinaryColor::On);

    let mut header: String<20> = String::new();
    // core::fmt::Write on heapless::String - infallible except on capacity
    // overflow; the widest string we build ("IR: 0xAA 0xBB") is 13 chars.
    let _ = write!(header, "IR: 0x{:02X} 0x{:02X}", cmd.address, cmd.command);

    let _ = Text::with_baseline(&header, Point::new(0, 0), style, Baseline::Top)
        .draw(display);
    let _ = Text::with_baseline(
        buttons::name_of(cmd.command),
        Point::new(0, 16),
        style,
        Baseline::Top,
    )
    .draw(display);

    let _ = display.flush();
}
