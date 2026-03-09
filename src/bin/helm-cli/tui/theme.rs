//! Kinfolk-inspired Minimalist Theme for Helm TUI.
//! "Breathable, asymmetric, warm."

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, BorderType};

// ── Kinfolk TUI Palette ──────────────────────────────────────────────────
// Main structure uses soft khakis and grays, completely avoiding neon.
pub const KHAKI_BASE: Color = Color::Rgb(215, 210, 195); // Light warm paper

pub const KHAKI_DARK: Color = Color::Rgb(165, 160, 140); // Structural borders
pub const KHAKI: Color = Color::Rgb(190, 185, 170);      // Mid-tone khaki
pub const CHARCOAL: Color = Color::Rgb(50, 50, 50);      // Primary text
pub const STONE: Color = Color::Rgb(140, 140, 140);      // Muted secondary text
pub const ORANGE: Color = Color::Rgb(220, 100, 30);      // The only accent color
pub const CREAM: Color = Color::Rgb(245, 240, 230);      // Light background
pub const CYAN: Color = Color::Rgb(80, 180, 180);        // Info/status
pub const FOREST: Color = Color::Rgb(60, 140, 80);       // Success/positive
pub const SAGE: Color = Color::Rgb(130, 160, 130);       // Subtle green
pub const VIOLET: Color = Color::Rgb(140, 100, 180);     // Accent secondary
pub const GOLD: Color = Color::Rgb(200, 170, 50);        // Warning/highlight
pub const RED: Color = Color::Rgb(200, 60, 60);          // Error/danger
pub const DIM_BG: Color = Color::Rgb(35, 35, 35);        // Dark background
pub const FOCUS_BORDER: Color = Color::Rgb(180, 170, 150); // Focused border
pub const IDLE_BORDER: Color = Color::Rgb(80, 80, 80);   // Idle border

// ── Shared Styles ────────────────────────────────────────────────────────
pub fn style_base() -> Style {
    Style::default().fg(CHARCOAL)
}

pub fn style_accent() -> Style {
    Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)
}

pub fn style_muted() -> Style {
    Style::default().fg(STONE)
}

// ── Borderless "Breathable" Blocks ───────────────────────────────────────
// Kinfolk design uses whitespace instead of boxes.
pub fn kinfolk_block<'a>(title: &'a str) -> Block<'a> {
    Block::default()
        .borders(Borders::TOP)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(KHAKI_DARK))
        .title(format!("  {}  ", title))
        .title_style(Style::default().fg(CHARCOAL).add_modifier(Modifier::BOLD))
}

pub fn kinfolk_margin() -> ratatui::layout::Margin {
    ratatui::layout::Margin { vertical: 1, horizontal: 4 }
}
