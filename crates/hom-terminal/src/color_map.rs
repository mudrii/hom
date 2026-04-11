//! Color mapping utilities between terminal emulation backends and ratatui.

use hom_core::TermColor;

/// Map a `TermColor` to a `ratatui::style::Color`.
pub fn term_color_to_ratatui(color: TermColor) -> ratatui::style::Color {
    use ratatui::style::Color;
    match color {
        TermColor::Default => Color::Reset,
        TermColor::Black => Color::Black,
        TermColor::Red => Color::Red,
        TermColor::Green => Color::Green,
        TermColor::Yellow => Color::Yellow,
        TermColor::Blue => Color::Blue,
        TermColor::Magenta => Color::Magenta,
        TermColor::Cyan => Color::Cyan,
        TermColor::White => Color::White,
        TermColor::BrightBlack => Color::DarkGray,
        TermColor::BrightRed => Color::LightRed,
        TermColor::BrightGreen => Color::LightGreen,
        TermColor::BrightYellow => Color::LightYellow,
        TermColor::BrightBlue => Color::LightBlue,
        TermColor::BrightMagenta => Color::LightMagenta,
        TermColor::BrightCyan => Color::LightCyan,
        TermColor::BrightWhite => Color::White,
        TermColor::Indexed(idx) => Color::Indexed(idx),
        TermColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::*;

    #[test]
    fn maps_named_and_bright_colors() {
        assert_eq!(term_color_to_ratatui(TermColor::Default), Color::Reset);
        assert_eq!(
            term_color_to_ratatui(TermColor::BrightBlack),
            Color::DarkGray
        );
        assert_eq!(term_color_to_ratatui(TermColor::BrightWhite), Color::White);
        assert_eq!(
            term_color_to_ratatui(TermColor::BrightCyan),
            Color::LightCyan
        );
    }

    #[test]
    fn maps_indexed_and_rgb_colors() {
        assert_eq!(
            term_color_to_ratatui(TermColor::Indexed(42)),
            Color::Indexed(42)
        );
        assert_eq!(
            term_color_to_ratatui(TermColor::Rgb(1, 2, 3)),
            Color::Rgb(1, 2, 3)
        );
    }
}
