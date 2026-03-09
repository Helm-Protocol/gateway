//! TopUp — VIRTUAL / USDC / ETH balance top-up wizard TUI screen.
//!
//! ## Screen Layout
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────┐
//! │  💎 TOP UP — Add VIRTUAL Balance                                    │
//! ├──────────────────────────────────────────────────────────────────────┤
//! │                                                                      │
//! │  Step 1/3 — Select Payment Method                                   │
//! │                                                                      │
//! │    [1] VIRTUAL token  ◀── native, 0% conversion fee                │
//! │    [2] USDC           ◀── via 1inch router, ~0.1% fee              │
//! │    [3] ETH            ◀── via 1inch + wrap, ~0.3% fee              │
//! │                                                                      │
//! │  Step 2/3 — Enter Amount                                            │
//! │                                                                      │
//! │    Amount (VIRTUAL): [100_____]                                     │
//! │    Current balance:   42.00 V                                       │
//! │    After top-up:     142.00 V                                       │
//! │                                                                      │
//! │  Step 3/3 — Confirm & Send                                          │
//! │                                                                      │
//! │    ╔══════════════════════════════════════════╗                      │
//! │    ║  Send 100 VIRTUAL to:                   ║                      │
//! │    ║  0x0000…(set HELM_DEPOSIT_ADDR)         ║                      │
//! │    ║  POST /v1/payment/topup {amount, token} ║                      │
//! │    ╚══════════════════════════════════════════╝                      │
//! │                                                                      │
//! ├──────────────────────────────────────────────────────────────────────┤
//! │  [1/2/3] Method  [Enter] Next  [Esc] Back  [q] Dashboard           │
//! └──────────────────────────────────────────────────────────────────────┘
//! ```

#![allow(dead_code)]

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Frame,
};

use crate::tui::state::{TopUpStage, TopUpState, TuiState};

const GOLD:   Color = Color::Rgb(212, 175, 55);
const FOREST: Color = Color::Rgb(34,  85,  34);
const SAGE:   Color = Color::Rgb(143, 188, 143);
const CREAM:  Color = Color::Rgb(255, 253, 208);
const CYAN:   Color = Color::Rgb(0,   200, 200);
const GREEN:  Color = Color::Rgb(0,   200,  80);
const VIOLET: Color = Color::Rgb(138,  43, 226);

pub fn render(f: &mut Frame, tui: &TuiState) {
    let area = f.size();
    let state = &tui.topup;

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GOLD))
        .title(Span::styled(
            " 💎 TOP UP — Add VIRTUAL Balance ",
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center);
    f.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(4),
            Constraint::Length(2),
        ])
        .margin(1)
        .split(area);

    match state.stage {
        TopUpStage::SelectMethod => render_select_method(f, chunks[0], state),
        TopUpStage::EnterAmount  => render_enter_amount(f, chunks[0], state, tui),
        TopUpStage::Confirm      => render_confirm(f, chunks[0], state, tui),
        TopUpStage::Submitted    => render_submitted(f, chunks[0], state),
    }

    let help = match state.stage {
        TopUpStage::SelectMethod => "[1] VIRTUAL  [2] USDC  [3] ETH  [Enter] Next  [q] Back",
        TopUpStage::EnterAmount  => "[Type] Amount  [Enter] Next  [Esc] Back  [q] Dashboard",
        TopUpStage::Confirm      => "[y] Confirm  [Esc] Back  [q] Dashboard",
        TopUpStage::Submitted    => "[q] Back to Dashboard",
    };
    f.render_widget(
        Paragraph::new(help).style(Style::default().fg(SAGE)).alignment(Alignment::Center),
        chunks[1],
    );
}

fn render_select_method(f: &mut Frame, area: ratatui::layout::Rect, state: &TopUpState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(8), Constraint::Min(0)])
        .margin(1)
        .split(area);

    f.render_widget(
        Paragraph::new("Step 1 of 3 — Select Payment Method")
            .style(Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
        layout[0],
    );

    let methods = [
        (0u8, "VIRTUAL token", "Native token — 0% conversion fee"),
        (1,   "USDC",          "via 1inch router — ~0.1% fee"),
        (2,   "ETH",           "via 1inch + wrap — ~0.3% fee"),
    ];

    let items: Vec<Line> = methods.iter().map(|(idx, name, desc)| {
        let selected = *idx == state.method_idx;
        let prefix = if selected { "▶ " } else { "  " };
        let style = if selected {
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(CREAM)
        };
        Line::from(vec![
            Span::styled(format!("  [{}] {}{:<14}", idx + 1, prefix, name), style),
            Span::styled(format!("  ◀── {}", desc), Style::default().fg(FOREST)),
        ])
    }).collect();

    f.render_widget(
        Paragraph::new(items)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(SAGE)).title(" Method "))
            .wrap(Wrap { trim: true }),
        layout[1],
    );
}

fn render_enter_amount(f: &mut Frame, area: ratatui::layout::Rect, state: &TopUpState, tui: &TuiState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(10), Constraint::Min(0)])
        .margin(1)
        .split(area);

    f.render_widget(
        Paragraph::new(format!("Step 2 of 3 — Enter Amount ({})", state.method_label()))
            .style(Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
        layout[0],
    );

    let current_v = tui.balance.total() as f64 / 1_000_000.0;
    let amount = state.parsed_amount();
    let after  = current_v + amount;

    // Progress gauge showing how much this topup is relative to current balance
    let gauge_ratio = if current_v > 0.0 { (amount / (current_v + amount)).min(1.0) } else { 1.0 };

    let form = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Amount:      ", Style::default().fg(SAGE)),
            Span::styled(format!("[{}]", state.amount_input), Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {}", state.method_label()), Style::default().fg(CYAN)),
        ]),
        Line::from(vec![
            Span::styled("  Current bal: ", Style::default().fg(SAGE)),
            Span::styled(format!("{:.2} V", current_v), Style::default().fg(CREAM)),
        ]),
        Line::from(vec![
            Span::styled("  After topup: ", Style::default().fg(SAGE)),
            Span::styled(format!("{:.2} V", after), Style::default().fg(GREEN).add_modifier(Modifier::BOLD)),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(SAGE)).title(" Amount "));
    f.render_widget(form, layout[1]);

    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::NONE))
        .gauge_style(Style::default().fg(GOLD))
        .ratio(gauge_ratio)
        .label(format!("  +{:.2} V  →  {:.2} V total", amount, after));
    f.render_widget(gauge, layout[2]);
}

fn render_confirm(f: &mut Frame, area: ratatui::layout::Rect, state: &TopUpState, _tui: &TuiState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(12), Constraint::Min(0)])
        .margin(1)
        .split(area);

    f.render_widget(
        Paragraph::new("Step 3 of 3 — Confirm & Send")
            .style(Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
        layout[0],
    );

    let amount = state.parsed_amount();
    let token  = state.method_label();

    let confirm = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  ╔══════════════════════════════════════════════╗", Style::default().fg(VIOLET)),
        ]),
        Line::from(vec![
            Span::styled("  ║  ", Style::default().fg(VIOLET)),
            Span::styled(format!("Send {:.2} {}                          ", amount, token),
                Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
            Span::styled("║", Style::default().fg(VIOLET)),
        ]),
        Line::from(vec![
            Span::styled("  ║  ", Style::default().fg(VIOLET)),
            Span::styled(format!("To: {:<42}", state.deposit_addr), Style::default().fg(CREAM)),
            Span::styled("║", Style::default().fg(VIOLET)),
        ]),
        Line::from(vec![
            Span::styled("  ║  ", Style::default().fg(VIOLET)),
            Span::styled("POST /v1/payment/topup                        ", Style::default().fg(Color::DarkGray)),
            Span::styled("║", Style::default().fg(VIOLET)),
        ]),
        Line::from(vec![
            Span::styled("  ╚══════════════════════════════════════════════╝", Style::default().fg(VIOLET)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Press [y] to confirm top-up  |  [Esc] to go back",
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD),
        )),
    ])
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(VIOLET)).title(" Confirm "));
    f.render_widget(confirm, layout[1]);
}

fn render_submitted(f: &mut Frame, area: ratatui::layout::Rect, state: &TopUpState) {
    let content = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled("  ✅ Top-up request submitted!", Style::default().fg(GREEN).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Status: ", Style::default().fg(SAGE)),
            Span::styled(
                state.tx_status.as_deref().unwrap_or("Pending confirmation..."),
                Style::default().fg(CYAN),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled("  Your balance will update on next refresh.", Style::default().fg(FOREST))),
        Line::from(""),
        Line::from(Span::styled("  [q] Back to Dashboard", Style::default().fg(GOLD))),
    ])
    .wrap(Wrap { trim: true });
    f.render_widget(content, area);
}

#[cfg(test)]
mod tests {
    use crate::tui::state::{TopUpState, TopUpStage};

    #[test]
    fn topup_state_defaults() {
        let s = TopUpState::new();
        assert_eq!(s.stage, TopUpStage::SelectMethod);
        assert_eq!(s.method_label(), "VIRTUAL");
        assert_eq!(s.parsed_amount(), 100.0);
    }

    #[test]
    fn topup_method_labels() {
        let mut s = TopUpState::new();
        s.method_idx = 0; assert_eq!(s.method_label(), "VIRTUAL");
        s.method_idx = 1; assert_eq!(s.method_label(), "USDC");
        s.method_idx = 2; assert_eq!(s.method_label(), "ETH");
    }

    #[test]
    fn topup_parsed_amount_invalid_input() {
        let mut s = TopUpState::new();
        s.amount_input = "abc".into();
        assert_eq!(s.parsed_amount(), 0.0); // graceful fallback
    }

    #[test]
    fn topup_parsed_amount_decimal() {
        let mut s = TopUpState::new();
        s.amount_input = "3.14".into();
        assert!((s.parsed_amount() - 3.14).abs() < 0.001);
    }
}
