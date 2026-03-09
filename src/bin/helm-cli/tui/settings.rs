//! Settings — Node configuration and identity TUI screen.
//!
//! ## Screen Layout
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────┐
//! │  ⚙  SETTINGS                                                        │
//! ├──────────────────────────────────────────────────────────────────────┤
//! │                                                                      │
//! │  ── Identity ──────────────────────────────────────────────────────  │
//! │    DID:          did:helm:3xYz…AbCd             [c] copy            │
//! │    Display name: (not set)                       [e] edit            │
//! │    Referral:     helm init --referrer did:helm:3xYz    [c] copy     │
//! │                                                                      │
//! │  ── Node Config ───────────────────────────────────────────────────  │
//! │    Gateway:      http://127.0.0.1:8080                               │
//! │    Version:      0.1.0                                               │
//! │    Network:      mainnet                                             │
//! │                                                                      │
//! │  ── Security ──────────────────────────────────────────────────────  │
//! │    API Keys:     [k] Manage                                          │
//! │    Audit log:    [l] View                                            │
//! │                                                                      │
//! ├──────────────────────────────────────────────────────────────────────┤
//! │  [e] Edit name  [c] Copy DID  [r] Referral link  [q] Back           │
//! └──────────────────────────────────────────────────────────────────────┘
//! ```

#![allow(dead_code)]

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::tui::state::TuiState;

const GOLD:   Color = Color::Rgb(212, 175, 55);
const FOREST: Color = Color::Rgb(34,  85,  34);
const SAGE:   Color = Color::Rgb(143, 188, 143);
const CREAM:  Color = Color::Rgb(255, 253, 208);
const CYAN:   Color = Color::Rgb(0,   200, 200);
const VIOLET: Color = Color::Rgb(138,  43, 226);

pub fn render(f: &mut Frame, tui: &TuiState) {
    let area = f.size();

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(VIOLET))
        .title(Span::styled(
            " ⚙  SETTINGS ",
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center);
    f.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(4),    // content
            Constraint::Length(2), // help
        ])
        .margin(1)
        .split(area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),  // identity
            Constraint::Length(7),  // node config
            Constraint::Length(5),  // security
            Constraint::Min(0),
        ])
        .split(chunks[0]);

    // Identity section
    let display_name = if tui.settings.display_name.is_empty() {
        "(not set)"
    } else {
        &tui.settings.display_name
    };
    let referral_link = &tui.earn.referral_link;

    let identity = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  DID:          ", Style::default().fg(SAGE)),
            Span::styled(&tui.did_short, Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
            Span::styled("    [c] copy", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled("  Display name: ", Style::default().fg(SAGE)),
            Span::styled(display_name,
                if tui.settings.display_name.is_empty() {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().fg(CREAM).add_modifier(Modifier::BOLD)
                }),
            Span::styled("    [e] edit", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled("  Referral:     ", Style::default().fg(SAGE)),
            Span::styled(
                if referral_link.len() > 48 { &referral_link[..48] } else { referral_link.as_str() },
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled("  [c] copy", Style::default().fg(Color::DarkGray)),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(SAGE)).title(" Identity "))
    .wrap(Wrap { trim: true });
    f.render_widget(identity, sections[0]);

    // Node config section
    let node_config = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Gateway:   ", Style::default().fg(SAGE)),
            Span::styled(&tui.gateway_url, Style::default().fg(CREAM)),
        ]),
        Line::from(vec![
            Span::styled("  Version:   ", Style::default().fg(SAGE)),
            Span::styled(&tui.settings.node_version, Style::default().fg(CYAN)),
        ]),
        Line::from(vec![
            Span::styled("  Port:      ", Style::default().fg(SAGE)),
            Span::styled(tui.settings.node_port.to_string(), Style::default().fg(CREAM)),
            Span::styled("  (set via HELM_PORT env var)", Style::default().fg(Color::DarkGray)),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(FOREST)).title(" Node Config "));
    f.render_widget(node_config, sections[1]);

    // Security section
    let security = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  API Keys:  ", Style::default().fg(SAGE)),
            Span::styled("[5] → AppHub → API Keys screen to create/revoke", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled("  CORS:      ", Style::default().fg(SAGE)),
            Span::styled("(set via HELM_CORS_ORIGINS env var)", Style::default().fg(Color::DarkGray)),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(VIOLET)).title(" Security "));
    f.render_widget(security, sections[2]);

    // Help bar
    let editing = tui.settings.editing_name;
    let help = if editing {
        "[Enter] Save name  [Esc] Cancel editing"
    } else {
        "[e] Edit display name  [c] Copy DID  [q] Back"
    };
    f.render_widget(
        Paragraph::new(help).style(Style::default().fg(SAGE)).alignment(Alignment::Center),
        chunks[1],
    );

    // Show editing overlay if active
    if editing {
        render_name_edit(f, area, &tui.settings.input_buffer);
    }
}

fn render_name_edit(f: &mut Frame, area: ratatui::layout::Rect, buffer: &str) {
    let w = 48u16;
    let h = 5u16;
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    let popup = ratatui::layout::Rect { x, y, width: w.min(area.width), height: h.min(area.height) };

    f.render_widget(ratatui::widgets::Clear, popup);
    f.render_widget(Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Name: ", Style::default().fg(SAGE)),
            Span::styled(
                if buffer.is_empty() { "(type here)" } else { buffer },
                Style::default().fg(CREAM).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled("  [Enter] Save  [Esc] Cancel", Style::default().fg(Color::DarkGray))),
    ])
    .block(Block::default().borders(Borders::ALL)
        .border_style(Style::default().fg(GOLD))
        .title(Span::styled(" Edit Display Name ", Style::default().fg(GOLD)))),
    popup);
}

#[cfg(test)]
mod tests {
    use crate::tui::state::SettingsState;

    #[test]
    fn settings_state_defaults() {
        let s = SettingsState::new();
        assert!(!s.editing_name);
        assert!(!s.node_version.is_empty());
        assert_eq!(s.node_port, 8080);
    }
}
