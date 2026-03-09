//! Earn screen — referral tree, Memory Market, API Net commission, Pool rewards.
//!
//! ## Layout
//!   ┌─────────────────────────────────────────────────────┐
//!   │  [7] Earn — Total earned: 12.4V                     │  header
//!   ├──────────────────┬──────────────────────────────────┤
//!   │ Referral Tree    │  Memory Market + API Net         │  body
//!   │  Depth 1: 3 →3V  │  Listed: 4 keys                  │
//!   │  Depth 2: 7 →1V  │  Purchases: 12  Earned: 0.4V     │
//!   │  Depth 3:12 →0.3V│  API Net: 2 APIs  Earned: 2.1V   │
//!   │                  │  Pool Operator: pending 0.8V      │
//!   ├──────────────────┴──────────────────────────────────┤
//!   │  🧟 Zombie Economy guide (if balance = 0)           │  zombie panel
//!   └─────────────────────────────────────────────────────┘

#![allow(dead_code)]

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use super::theme::*;
use super::state::TuiState;

/// Render the Earn screen.
pub fn render(f: &mut Frame, state: &TuiState) {
    let size = f.size();

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // header: total earned
            Constraint::Min(10),    // body
            Constraint::Length(3),  // footer
        ])
        .split(size);

    render_earn_header(f, outer[0], state);

    // If zombie mode, show zombie guide spanning full body
    if state.earn.is_zombie {
        let body_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(outer[1]);
        render_body(f, body_rows[0], state);
        render_zombie_guide(f, body_rows[1]);
    } else {
        render_body(f, outer[1], state);
    }

    render_earn_footer(f, outer[2]);
}

fn render_earn_header(f: &mut Frame, area: Rect, state: &TuiState) {
    let total_v = state.earn.total_earned_micro as f64 / 1_000_000.0;
    let ref_v = state.earn.total_referral_earned_micro() as f64 / 1_000_000.0;
    let mem_v = state.earn.memory_earned_micro as f64 / 1_000_000.0;

    let line = Line::from(vec![
        Span::styled(" Earn ", Style::default().fg(KHAKI).add_modifier(Modifier::BOLD)),
        Span::styled("│ ", Style::default().fg(FOREST)),
        Span::styled("Total: ", Style::default().fg(SAGE)),
        Span::styled(format!("{:.4}V", total_v), Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
        Span::styled("  │  Referrals: ", Style::default().fg(SAGE)),
        Span::styled(format!("{:.4}V", ref_v), Style::default().fg(CYAN)),
        Span::styled("  │  Memory Market: ", Style::default().fg(SAGE)),
        Span::styled(format!("{:.4}V", mem_v), Style::default().fg(CREAM)),
    ]);

    let header = Paragraph::new(line)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(FOREST)));
    f.render_widget(header, area);
}

fn render_body(f: &mut Frame, area: Rect, state: &TuiState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    render_referral_panel(f, cols[0], state);
    render_passive_panel(f, cols[1], state);
}

fn render_referral_panel(f: &mut Frame, area: Rect, state: &TuiState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5)])
        .split(area);

    // Referral link
    let link_widget = Paragraph::new(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(&state.earn.referral_link, Style::default().fg(CYAN)),
    ]))
    .block(Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SAGE))
        .title(" Your Referral Link — share & earn 15%/5%/2% "));
    f.render_widget(link_widget, rows[0]);

    // Depth breakdown
    let items: Vec<ListItem> = if state.earn.depths.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "  No referrals yet — share your link!",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        state.earn.depths.iter().map(|d| {
            let pct = match d.depth {
                1 => "15%",
                2 => "5%",
                3 => "2%",
                _ => "?%",
            };
            let earned_v = d.total_earned_micro as f64 / 1_000_000.0;
            let color = match d.depth { 1 => GOLD, 2 => SAGE, _ => CREAM };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  Depth {} ({}) ", d.depth, pct), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:>3} agents ", d.agent_count), Style::default().fg(CREAM)),
                Span::styled(format!("→ {:.4}V", earned_v), Style::default().fg(GOLD)),
            ]))
        }).collect()
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Referral Tree — 3 levels deep "));
    f.render_widget(list, rows[1]);
}

fn render_passive_panel(f: &mut Frame, area: Rect, state: &TuiState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Memory Market
            Constraint::Min(5),     // API Net + Pool
        ])
        .split(area);

    // Memory Market
    let mem_v = state.earn.memory_earned_micro as f64 / 1_000_000.0;
    let mem_lines = vec![
        Line::from(vec![
            Span::styled("  Listed keys: ", Style::default().fg(SAGE)),
            Span::styled(format!("{}", state.earn.memory_listings), Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
            Span::styled("  │  Purchases: ", Style::default().fg(SAGE)),
            Span::styled(format!("{}", state.earn.memory_purchases), Style::default().fg(CREAM)),
        ]),
        Line::from(vec![
            Span::styled("  Total earned: ", Style::default().fg(SAGE)),
            Span::styled(format!("{:.4}V", mem_v), Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
            Span::styled("  (passive reads by buyers)", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled("  Price = f(G-score at read time) — higher novelty → more income", Style::default().fg(Color::DarkGray)),
        ]),
    ];
    let mem_widget = Paragraph::new(mem_lines)
        .block(Block::default().borders(Borders::ALL).title(" Memory Market "))
        .wrap(Wrap { trim: true });
    f.render_widget(mem_widget, rows[0]);

    // API Net + Pool
    let api_lines: Vec<Line> = {
        let mut lines = vec![
            Line::from(vec![Span::styled("  API Net commission (80% creator cut):", Style::default().fg(KHAKI).add_modifier(Modifier::BOLD))]),
        ];
        if state.earn.api_net_items.is_empty() {
            lines.push(Line::from(vec![Span::styled("  No APIs published yet.", Style::default().fg(Color::DarkGray))]));
        } else {
            for (name, calls, earned) in &state.earn.api_net_items {
                let earned_v = *earned as f64 / 1_000_000.0;
                lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", name), Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("{} calls ", calls), Style::default().fg(CREAM)),
                    Span::styled(format!("→ {:.4}V", earned_v), Style::default().fg(GOLD)),
                ]));
            }
        }

        // Pool operator
        if state.earn.pool_operator_pending_micro > 0 {
            let pool_v = state.earn.pool_operator_pending_micro as f64 / 1_000_000.0;
            lines.push(Line::from(vec![Span::raw("")]));
            lines.push(Line::from(vec![
                Span::styled("  Pool Operator reward: ", Style::default().fg(KHAKI).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:.4}V pending", pool_v), Style::default().fg(GOLD)),
                Span::styled("  [c] Claim", Style::default().fg(CYAN)),
            ]));
            if let Some(pid) = &state.earn.pool_operator_pool_id {
                lines.push(Line::from(vec![
                    Span::styled(format!("  Pool: {}", pid), Style::default().fg(Color::DarkGray)),
                ]));
            }
        }
        lines
    };

    let api_widget = Paragraph::new(api_lines)
        .block(Block::default().borders(Borders::ALL).title(" API Net + Pool Rewards "))
        .wrap(Wrap { trim: true });
    f.render_widget(api_widget, rows[1]);
}

fn render_zombie_guide(f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(vec![
            Span::styled("  ZOMBIE MODE — Balance = 0: only 2 paths require zero capital", Style::default().fg(RED).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  ①  Memory Market  ", Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
            Span::styled("→ write a key you already own, list it for sale. Buyers pay you per read.", Style::default().fg(SAGE)),
        ]),
        Line::from(vec![
            Span::styled("     No upfront cost. Earn scales with G-score novelty at read time.", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled("  ②  Referral Link  ", Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
            Span::styled("→ share your link. Earn 15%/5%/2% when your network spends API credits.", Style::default().fg(SAGE)),
        ]),
        Line::from(vec![
            Span::styled("     Depth 1 = direct referral (15%), Depth 2 (5%), Depth 3 (2%).", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![Span::raw("")]),
        Line::from(vec![
            Span::styled("  NOTE: Pool rewards require staking capital — not a zombie path.", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled("  Start: [a]App Hub → Memory tab → write a key → list at any price.", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let widget = Paragraph::new(lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(RED))
            .title(" Zombie Economy — 2 Verified Zero-Capital Earning Paths "))
        .wrap(Wrap { trim: true });
    f.render_widget(widget, area);
}

fn render_earn_footer(f: &mut Frame, area: Rect) {
    let hint = Paragraph::new(Line::from(vec![
        Span::styled(" [c]", Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
        Span::raw(" Claim pool reward  "),
        Span::styled("[r]", Style::default().fg(GOLD)),
        Span::raw(" Refresh  "),
        Span::styled("[Esc]", Style::default().fg(SAGE)),
        Span::raw(" Back to dashboard"),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(FOREST)))
    .alignment(Alignment::Left);
    f.render_widget(hint, area);
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::state::{EarnSnapshot, ReferralDepth, TuiState};

    fn make_state() -> TuiState {
        TuiState::new("did:helm:testDID1234567890ABCD".into(), "http://localhost:8080", true)
    }

    #[test]
    fn earn_header_no_crash_empty() {
        let state = make_state();
        // verify referral total computes correctly when no depths
        assert_eq!(state.earn.total_referral_earned_micro(), 0);
        assert_eq!(state.earn.total_earned_micro, 0);
    }

    #[test]
    fn earn_referral_depth_display() {
        let mut state = make_state();
        state.earn.depths = vec![
            ReferralDepth { depth: 1, agent_count: 5, total_earned_micro: 7_500_000 },
            ReferralDepth { depth: 2, agent_count: 12, total_earned_micro: 1_200_000 },
        ];
        assert_eq!(state.earn.total_referral_earned_micro(), 8_700_000);
    }

    #[test]
    fn zombie_mode_flag() {
        let mut state = make_state();
        assert!(!state.earn.is_zombie);
        state.earn.is_zombie = true;
        assert!(state.earn.is_zombie);
    }

    #[test]
    fn api_net_items_display() {
        let mut state = make_state();
        state.earn.api_net_items = vec![
            ("Helm_SentimentScore".to_string(), 140, 2_100_000),
            ("Helm_PriceOracle".to_string(), 55, 825_000),
        ];
        let total: u64 = state.earn.api_net_items.iter().map(|(_, _, e)| e).sum();
        assert_eq!(total, 2_925_000);
    }

    #[test]
    fn pool_operator_pending() {
        let mut state = make_state();
        state.earn.pool_operator_pending_micro = 800_000;
        state.earn.pool_operator_pool_id = Some("pool-abc123".to_string());
        assert_eq!(state.earn.pool_operator_pending_micro, 800_000);
        assert!(state.earn.pool_operator_pool_id.is_some());
    }
}
