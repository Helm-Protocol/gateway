//! TUI Layout Utilities for scroll position and focused borders.

use ratatui::widgets::{Block, Borders, BorderType};
use ratatui::style::{Style, Modifier};
use crate::tui::theme::*;

pub struct UiUtils;

impl UiUtils {
    /// Generate a block with dynamic border based on focus.
    pub fn bordered_block(title: &str, is_focused: bool) -> Block<'static> {
        let style = if is_focused {
            Style::default().fg(FOCUS_BORDER).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(IDLE_BORDER)
        };

        Block::default()
            .borders(Borders::ALL)
            .border_type(if is_focused { BorderType::Thick } else { BorderType::Plain })
            .border_style(style)
            .title(format!(" {} ", title))
    }

    /// Generate a title with scroll position: " Title (N/M) "
    pub fn scroll_title(title: &str, current: usize, total: usize) -> String {
        if total == 0 {
            format!(" {} (0/0) ", title)
        } else {
            format!(" {} ({}/{}) ", title, current + 1, total)
        }
    }
}
