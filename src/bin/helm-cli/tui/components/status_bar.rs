//! Fixed Top Status Bar (k9s/lazygit style).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;
use ratatui::text::{Line, Span};
use ratatui::style::{Style, Modifier};
use crate::tui::state::{TuiState, MembershipTierExt};
use crate::tui::theme::*;

pub fn render(f: &mut Frame, area: Rect, state: &TuiState) {
    let tick_char = match state.tick % 4 {
        0 => "⠋", 1 => "⠙", 2 => "⠹", _ => "⠸",
    };

    let breadcrumb = match state.breadcrumb.last() {
        Some(b) => format!(" ⚓ Helm > {} ", b),
        None => " ⚓ Helm ".to_string(),
    };

    let line = Line::from(vec![
        Span::styled(breadcrumb, Style::default().fg(KHAKI).add_modifier(Modifier::BOLD)),
        Span::styled(" │ ", Style::default().fg(FOREST)),
        Span::styled(&state.did_short, Style::default().fg(CREAM)),
        Span::styled(" │ ", Style::default().fg(FOREST)),
        Span::styled(format!(" Tier: {} ", state.score.tier.label()), Style::default().fg(ORANGE)),
        Span::styled(" │ ", Style::default().fg(FOREST)),
        Span::styled(format!(" {:.2}V ", state.balance.total_v()), Style::default().fg(CYAN)),
        Span::styled(" │ ", Style::default().fg(FOREST)),
        Span::styled(format!(" {} HELM ", state.balance.helm_count()), Style::default().fg(GOLD)),
        Span::styled(format!(" {} live", tick_char), Style::default().fg(SAGE)),
    ]);

    let bar = Paragraph::new(line)
        .block(crate::tui::components::utils::UiUtils::bordered_block("", false))
        .style(Style::default().bg(DIM_BG));

    f.render_widget(bar, area);
}
