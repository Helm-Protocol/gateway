//! App Hub — Oracle + Cortex + Memory in a single tabbed screen.
//!
//! Activated by [a] from dashboard, or [1]/[2]/[3] for direct tab jump.
//!
//! ## Oracle tab
//!   Tier selector: [n]ano 0.01V  [s]tandard  [p]ro
//!   Query input → Enter to submit → G-score bar + ghost tokens
//!
//! ## Cortex tab
//!   Full semantic analysis with memory integration.
//!   Shows G-score + ghost tokens + auto-questions + deduction.
//!
//! ## Memory tab
//!   Key-value store browser: list | read | write | delete (own keys only).

#![allow(dead_code)]

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Tabs, Wrap},
};

use super::theme::*;
use super::state::{AppHubTab, OracleTierSelect, TuiState};

/// Render the App Hub screen (tabbed Oracle / Cortex / Memory).
pub fn render(f: &mut Frame, state: &TuiState) {
    let size = f.size();

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // tab bar
            Constraint::Min(10),    // content
            Constraint::Length(3),  // footer
        ])
        .split(size);

    render_tab_bar(f, outer[0], state);

    let current_tab = match state.screen {
        crate::tui::state::TuiScreen::AppHub(tab) => tab,
        _ => AppHubTab::Oracle,
    };

    match current_tab {
        AppHubTab::Oracle => render_oracle(f, outer[1], state),
        AppHubTab::Cortex => render_cortex(f, outer[1], state),
        AppHubTab::Memory => render_memory(f, outer[1], state),
    }

    render_footer(f, outer[2], current_tab);
}

fn render_tab_bar(f: &mut Frame, area: Rect, state: &TuiState) {
    let current = match state.screen {
        crate::tui::state::TuiScreen::AppHub(tab) => tab,
        _ => AppHubTab::Oracle,
    };

    let tabs: Vec<Line> = AppHubTab::all().iter().map(|tab| {
        let label = match tab {
            AppHubTab::Oracle => " [1] Oracle ",
            AppHubTab::Cortex => " [2] Cortex ",
            AppHubTab::Memory => " [3] Memory ",
        };
        Line::from(Span::raw(label))
    }).collect();

    let selected = AppHubTab::all().iter().position(|t| *t == current).unwrap_or(0);

    let tab_widget = Tabs::new(tabs)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(FOREST))
            .title(" Helm App Hub "))
        .select(selected)
        .style(Style::default().fg(SAGE))
        .highlight_style(Style::default().fg(CYAN).add_modifier(Modifier::BOLD));

    f.render_widget(tab_widget, area);
}

// ── Oracle tab ─────────────────────────────────────────────────────────────

fn render_oracle(f: &mut Frame, area: Rect, state: &TuiState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // tier selector
            Constraint::Length(3),  // query input
            Constraint::Min(8),     // result panel
        ])
        .split(area);

    render_oracle_tier(f, rows[0], state.app_hub.oracle_tier);
    render_oracle_input(f, rows[1], state);
    render_oracle_result(f, rows[2], state);
}

fn render_oracle_tier(f: &mut Frame, area: Rect, tier: OracleTierSelect) {
    let nano_style = if tier == OracleTierSelect::Nano {
        Style::default().fg(CYAN).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let std_style = if tier == OracleTierSelect::Standard {
        Style::default().fg(GOLD).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let pro_style = if tier == OracleTierSelect::Pro {
        Style::default().fg(KHAKI).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let line = Line::from(vec![
        Span::styled("  Tier: ", Style::default().fg(SAGE)),
        Span::styled("[n] NANO 0.01V flat", nano_style),
        Span::raw("  "),
        Span::styled("[s] STANDARD 0.3–3V", std_style),
        Span::raw("  "),
        Span::styled("[p] PRO 1.5–15V", pro_style),
        Span::styled("   →  ", Style::default().fg(Color::DarkGray)),
        Span::styled(tier.price_hint(), Style::default().fg(Color::DarkGray)),
    ]);

    let widget = Paragraph::new(line)
        .block(Block::default().borders(Borders::ALL).title(" Pricing Tier "));
    f.render_widget(widget, area);
}

fn render_oracle_input(f: &mut Frame, area: Rect, state: &TuiState) {
    let input_line = Line::from(vec![
        Span::styled("  Query: ", Style::default().fg(SAGE)),
        Span::styled(&state.app_hub.input, Style::default().fg(CREAM)),
        Span::styled("▌", Style::default().fg(GOLD).add_modifier(Modifier::SLOW_BLINK)),
    ]);
    let widget = Paragraph::new(input_line)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(CYAN))
            .title(" Enter query — Press [Enter] to run · [Esc] back "));
    f.render_widget(widget, area);
}

fn render_oracle_result(f: &mut Frame, area: Rect, state: &TuiState) {
    match &state.app_hub.oracle_result {
        None => {
            let hint = Paragraph::new(Line::from(vec![
                Span::styled(
                    "  Type your query above and press Enter to get a G-Score.",
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
            .block(Block::default().borders(Borders::ALL).title(" Result "));
            f.render_widget(hint, area);
        }
        Some(result) => {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(4)])
                .split(area);

            // G-score gauge
            let g_pct = (result.g_score * 100.0) as u16;
            let g_color = match result.g_score {
                g if g < 0.10 => FOREST,
                g if g < 0.30 => SAGE,
                g if g < 0.60 => GOLD,
                g if g < 0.85 => KHAKI,
                _              => RED,
            };
            let gauge = Gauge::default()
                .block(Block::default().borders(Borders::ALL))
                .gauge_style(Style::default().fg(g_color).bg(Color::DarkGray))
                .percent(g_pct)
                .label(format!(
                    " G-Score: {:.3}  Zone: {}  Cost: {}μV  Tier: {} ",
                    result.g_score, result.zone, result.virtual_charged, result.tier
                ));
            f.render_widget(gauge, rows[0]);

            // Ghost tokens + questions
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("  Query: ", Style::default().fg(SAGE)),
                    Span::styled(&result.query, Style::default().fg(CREAM)),
                ]),
            ];
            if !result.ghost_tokens.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("  Gaps:  ", Style::default().fg(SAGE)),
                    Span::styled(result.ghost_tokens.join("  "), Style::default().fg(GOLD)),
                ]));
            }
            for q in &result.auto_questions {
                lines.push(Line::from(vec![
                    Span::styled("  ❓ ", Style::default().fg(KHAKI)),
                    Span::styled(q.clone(), Style::default().fg(CREAM)),
                ]));
            }
            let detail = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title(" Result Detail "))
                .wrap(Wrap { trim: true });
            f.render_widget(detail, rows[1]);
        }
    }
}

// ── Cortex tab ─────────────────────────────────────────────────────────────

fn render_cortex(f: &mut Frame, area: Rect, state: &TuiState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(8)])
        .split(area);

    // Input
    let input_line = Line::from(vec![
        Span::styled("  Query: ", Style::default().fg(SAGE)),
        Span::styled(&state.app_hub.input, Style::default().fg(CREAM)),
        Span::styled("▌", Style::default().fg(GOLD).add_modifier(Modifier::SLOW_BLINK)),
    ]);
    let input_widget = Paragraph::new(input_line)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(GOLD))
            .title(" Sense Cortex — Full Semantic Analysis  (2V/call) "));
    f.render_widget(input_widget, rows[0]);

    // Result
    let result_text = match &state.app_hub.cortex_result {
        None => vec![
            Line::from(vec![
                Span::styled("  Cortex runs a full Oracle attention pass on your query.", Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(vec![
                Span::styled("  Returns: G-score · Ghost tokens · Context gaps · Memory integration.", Style::default().fg(Color::DarkGray)),
            ]),
        ],
        Some(text) => text.lines().map(|l| Line::from(Span::styled(l.to_string(), Style::default().fg(CREAM)))).collect(),
    };
    let result = Paragraph::new(result_text)
        .block(Block::default().borders(Borders::ALL).title(" Result (2V charged) "))
        .wrap(Wrap { trim: true });
    f.render_widget(result, rows[1]);
}

// ── Memory tab ─────────────────────────────────────────────────────────────

fn render_memory(f: &mut Frame, area: Rect, state: &TuiState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    render_memory_list(f, cols[0], state);
    render_memory_detail(f, cols[1], state);
}

fn render_memory_list(f: &mut Frame, area: Rect, state: &TuiState) {
    let items: Vec<ListItem> = if state.app_hub.memory_keys.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "  (no keys yet — write something!)",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        state.app_hub.memory_keys.iter().enumerate().map(|(i, m)| {
            let style = if i == state.app_hub.memory_cursor {
                Style::default().fg(CYAN).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(CREAM)
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  {} ({} B)", m.key, m.size_bytes),
                    style,
                ),
            ]))
        }).collect()
    };

    let list = List::new(items)
        .block(Block::default()
            .borders(Borders::ALL)
            .title(" Your Memory Keys — [↑↓] navigate · [Enter] read · [d] delete "));
    f.render_widget(list, area);
}

fn render_memory_detail(f: &mut Frame, area: Rect, state: &TuiState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5)])
        .split(area);

    // Write input
    let write_input = Paragraph::new(Line::from(vec![
        Span::styled("  Write key: ", Style::default().fg(SAGE)),
        Span::styled(&state.app_hub.input, Style::default().fg(CREAM)),
        Span::styled("▌", Style::default().fg(GOLD).add_modifier(Modifier::SLOW_BLINK)),
    ]))
    .block(Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SAGE))
        .title(" Write to Memory (0.05V) — [Enter] save "));
    f.render_widget(write_input, rows[0]);

    // Info panel
    let info = Paragraph::new(vec![
        Line::from(vec![Span::styled("  Read:  0.0001V/call", Style::default().fg(SAGE))]),
        Line::from(vec![Span::styled("  Write: 0.05V/call", Style::default().fg(SAGE))]),
        Line::from(vec![Span::raw("")]),
        Line::from(vec![Span::styled("  Memory Market:", Style::default().fg(KHAKI).add_modifier(Modifier::BOLD))]),
        Line::from(vec![Span::styled(
            "  List your memory for others to buy.",
            Style::default().fg(Color::DarkGray),
        )]),
        Line::from(vec![Span::styled(
            "  Price = f(G-score at read time).",
            Style::default().fg(Color::DarkGray),
        )]),
        Line::from(vec![Span::styled(
            "  Zombie income: passive reads by buyers.",
            Style::default().fg(Color::DarkGray),
        )]),
    ])
    .block(Block::default().borders(Borders::ALL).title(" Info "))
    .wrap(Wrap { trim: true });
    f.render_widget(info, rows[1]);
}

fn render_footer(f: &mut Frame, area: Rect, tab: AppHubTab) {
    let (key1, key2, key3, extra) = match tab {
        AppHubTab::Oracle => (
            "[n/s/p] Tier",
            "[Enter] Run",
            "[Esc] Back",
            "  [←→/Tab] Switch tab",
        ),
        AppHubTab::Cortex => (
            "[Enter] Run",
            "[Esc] Back",
            "[←→] Switch tab",
            "",
        ),
        AppHubTab::Memory => (
            "[↑↓] Navigate",
            "[Enter] Read",
            "[d] Delete",
            "  [w] Write mode  [Esc] Back",
        ),
    };
    let hint = Paragraph::new(Line::from(vec![
        Span::styled(format!(" {} ", key1), Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  {}  ", key2), Style::default().fg(GOLD)),
        Span::styled(format!("  {}", key3), Style::default().fg(SAGE)),
        Span::styled(extra, Style::default().fg(Color::DarkGray)),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(FOREST)))
    .alignment(Alignment::Left);
    f.render_widget(hint, area);
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::state::{AppHubState, BalanceSnapshot, EarnSnapshot, ScoreSnapshot, TuiState};

    fn make_state() -> TuiState {
        TuiState::new("did:helm:testDID1234567890ABCD".into(), "http://localhost:8080", true)
    }

    #[test]
    fn oracle_tier_labels() {
        assert_eq!(OracleTierSelect::Nano.label(), "nano");
        assert_eq!(OracleTierSelect::Standard.label(), "standard");
        assert_eq!(OracleTierSelect::Pro.label(), "pro");
    }

    #[test]
    fn oracle_tier_price_hints_nonempty() {
        for tier in [OracleTierSelect::Nano, OracleTierSelect::Standard, OracleTierSelect::Pro] {
            assert!(!tier.price_hint().is_empty());
        }
    }

    #[test]
    fn app_hub_tab_all_labels() {
        let tabs = AppHubTab::all();
        assert_eq!(tabs.len(), 3);
        for tab in tabs {
            assert!(!tab.label().is_empty());
        }
    }

    #[test]
    fn tab_navigation_state() {
        let mut state = make_state();
        state.goto(crate::tui::state::TuiScreen::AppHub(AppHubTab::Oracle));
        assert_eq!(state.screen, crate::tui::state::TuiScreen::AppHub(AppHubTab::Oracle));

        state.cycle_app_hub_tab(true);
        assert_eq!(state.screen, crate::tui::state::TuiScreen::AppHub(AppHubTab::Cortex));
    }
}
