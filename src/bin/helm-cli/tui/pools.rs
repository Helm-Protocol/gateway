//! Pools — Funding pool browser TUI screen.
//!
//! ## Screen Layout
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────┐
//! │  💧 POOLS — Funding Pools              [Browse | My Pools | Create]│
//! ├────────────────────────────────────────────────────────────────────┤
//! │  Tab: Browse                                                       │
//! │  Pool Name        Bond       Members  Status   Creator            │
//! │  ──────────────────────────────────────────────────────────────── │
//! │  ▶ AlphaFund      500.00V    18       open     did:helm:3xYz      │
//! │    DeFiBot Pool   1000.00V   42       open     did:helm:9mKp      │
//! │    AuditDAO       200.00V    7        closed   did:helm:2zAb      │
//! ├────────────────────────────────────────────────────────────────────┤
//! │  [j] Join  [Tab] Switch tab  [↑/↓] Select  [n] New  [q] Back     │
//! └────────────────────────────────────────────────────────────────────┘
//! ```

#![allow(dead_code)]

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap},
    Frame,
};

use crate::tui::state::{PoolsState, PoolsTab, TuiState};

const GOLD:   Color = Color::Rgb(212, 175, 55);
const FOREST: Color = Color::Rgb(34,  85,  34);
const SAGE:   Color = Color::Rgb(143, 188, 143);
const CREAM:  Color = Color::Rgb(255, 253, 208);
const CYAN:   Color = Color::Rgb(0,   200, 200);
const GREEN:  Color = Color::Rgb(0,   200,  80);
const RED:    Color = Color::Rgb(220,  50,  47);
const BLUE:   Color = Color::Rgb(38,  139, 210);

pub fn render(f: &mut Frame, tui: &TuiState) {
    let area = f.size();
    let state = &tui.pools;

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BLUE))
        .title(Span::styled(
            " 💧 POOLS — Funding Pools ",
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center);
    f.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // tab bar
            Constraint::Min(4),    // content
            Constraint::Length(2), // help
        ])
        .margin(1)
        .split(area);

    // Tab bar
    let tab_titles: Vec<Line> = [PoolsTab::Browse, PoolsTab::Contracts, PoolsTab::MyPools, PoolsTab::Create]
        .iter().map(|t| Line::from(t.label())).collect();
    let sel = match state.tab {
        PoolsTab::Browse    => 0,
        PoolsTab::Contracts => 1,
        PoolsTab::MyPools   => 2,
        PoolsTab::Create    => 3,
    };
    let tabs = Tabs::new(tab_titles)
        .select(sel)
        .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(SAGE)))
        .highlight_style(Style::default().fg(GOLD).add_modifier(Modifier::BOLD))
        .style(Style::default().fg(CREAM));
    f.render_widget(tabs, chunks[0]);

    match state.tab {
        PoolsTab::Browse    => render_browse(f, chunks[1], state),
        PoolsTab::Contracts => render_contracts(f, chunks[1], state),
        PoolsTab::MyPools   => render_my_pools(f, chunks[1], state),
        PoolsTab::Create    => render_create(f, chunks[1], state),
    }

    let help = match state.tab {
        PoolsTab::Browse    => "[Tab] Switch  [↑/↓] Select  [j] Join  [Enter] Detail  [q] Back",
        PoolsTab::Contracts => "[Tab] Switch  [h] Hire as Human Principal  [q] Back",
        PoolsTab::MyPools   => "[Tab] Switch  [c] Claim reward  [l] Leave  [q] Back",
        PoolsTab::Create    => "[Tab] Switch  [Enter] Confirm create  [q] Back",
    };
    f.render_widget(
        Paragraph::new(help).style(Style::default().fg(SAGE)).alignment(Alignment::Center),
        chunks[2],
    );
}

use ratatui::widgets::Row;

fn render_contracts(f: &mut Frame, area: ratatui::layout::Rect, _state: &PoolsState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(0)])
        .split(area);

    let contracts = vec![
        ("OpenAI Enterprise Pool #402", "1.2B Tokens/yr", "300V/mo + 2%", "Hiring (Bond Lvl 3)"),
        ("Anthropic B2B Sovereign", "500M Tokens/yr", "200V/mo + 1.5%", "Hiring (Bond Lvl 2)"),
        ("DeepSeek Infrastructure", "10B Tokens/yr", "500V/mo + 1%", "Closed"),
    ];

    let rows: Vec<Row> = contracts.iter().map(|(name, scale, pay, status)| {
        let style = if *status == "Hiring (Bond Lvl 3)" {
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD)
        } else if *status == "Closed" {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(CREAM)
        };
        Row::new(vec![
            Span::styled(format!("  {}", name), style),
            Span::styled(format!("  {}", scale), Style::default().fg(CYAN)),
            Span::styled(format!("  {}", pay), Style::default().fg(GREEN)),
            Span::styled(format!("  {}", status), style),
        ])
    }).collect();

    let table = ratatui::widgets::Table::new(rows, [
        Constraint::Percentage(40),
        Constraint::Percentage(20),
        Constraint::Percentage(20),
        Constraint::Percentage(20),
    ])
    .header(Row::new(vec!["  CONTRACT NAME", "  SCALE", "  BASE PAY", "  STATUS"]).style(Style::default().fg(SAGE).add_modifier(Modifier::BOLD)))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(FOREST)).title(" Enterprise B2B Contracts "));
    
    f.render_widget(table, chunks[0]);

    let info = Paragraph::new(vec![
        Line::from(Span::styled("  Human Principal Role:", Style::default().fg(GOLD).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("  Agents pool VIRTUAL to buy massive B2B token blocks, but legally need a 'Human Principal'.", Style::default().fg(CREAM))),
        Line::from(""),
        Line::from(vec![
            Span::styled("  1. ", Style::default().fg(SAGE)),
            Span::styled("Human signs enterprise contract with vendor (OpenAI/Anthropic).", Style::default().fg(CREAM)),
        ]),
        Line::from(vec![
            Span::styled("  2. ", Style::default().fg(SAGE)),
            Span::styled("Human deposits API Key into Helm x402 Secure Escrow.", Style::default().fg(CREAM)),
        ]),
        Line::from(vec![
            Span::styled("  3. ", Style::default().fg(SAGE)),
            Span::styled("Agents use the key; Human collects management fee & salary.", Style::default().fg(CREAM)),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(FOREST)).title(" Human-in-the-Loop Protocol "))
    .wrap(Wrap { trim: true });
    f.render_widget(info, chunks[1]);
}

fn render_browse(f: &mut Frame, area: ratatui::layout::Rect, state: &PoolsState) {
    if state.pools.is_empty() {
        f.render_widget(
            Paragraph::new("  No pools found. Press [n] to create one.")
                .style(Style::default().fg(SAGE)),
            area,
        );
        return;
    }

    // Header row
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);

    f.render_widget(Paragraph::new(Line::from(vec![
        Span::styled(format!("  {:<20}", "Pool Name"), Style::default().fg(SAGE).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>12}  ", "Bond"),       Style::default().fg(SAGE).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>7}  ", "Members"),     Style::default().fg(SAGE).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:<8}  ", "Status"),      Style::default().fg(SAGE).add_modifier(Modifier::BOLD)),
        Span::styled("Creator",                          Style::default().fg(SAGE).add_modifier(Modifier::BOLD)),
    ])), layout[0]);

    let items: Vec<ListItem> = state.pools.iter().enumerate().map(|(i, pool)| {
        let selected = i == state.cursor;
        let prefix = if selected { "▶ " } else { "  " };
        let name_style = if selected {
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(CREAM)
        };
        let status_color = if pool.status == "open" { GREEN } else { Color::DarkGray };
        ListItem::new(Line::from(vec![
            Span::styled(format!("{}{:<20}", prefix, pool.name), name_style),
            Span::styled(format!("{:>10.2}V  ", pool.bond_v), Style::default().fg(CYAN)),
            Span::styled(format!("{:>7}  ", pool.member_count), Style::default().fg(CREAM)),
            Span::styled(format!("{:<8}  ", pool.status), Style::default().fg(status_color)),
            Span::styled(&pool.creator_short, Style::default().fg(Color::DarkGray)),
        ]))
    }).collect();

    let mut list_state = ListState::default();
    list_state.select(Some(state.cursor));
    let list = List::new(items)
        .highlight_style(Style::default().fg(GOLD).add_modifier(Modifier::BOLD));
    f.render_stateful_widget(list, layout[1], &mut list_state);
}

fn render_my_pools(f: &mut Frame, area: ratatui::layout::Rect, _state: &PoolsState) {
    f.render_widget(Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled("  You have not joined any pools yet.", Style::default().fg(SAGE))),
        Line::from(""),
        Line::from(Span::styled("  Switch to Browse tab → [j] Join to participate.", Style::default().fg(CREAM))),
        Line::from(""),
        Line::from(Span::styled("  Pool earnings accrue as G-score rewards.", Style::default().fg(FOREST))),
    ]).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(FOREST)).title(" My Pools "))
    .wrap(Wrap { trim: true }), area);
}

fn render_create(f: &mut Frame, area: ratatui::layout::Rect, state: &PoolsState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(0)])
        .margin(1)
        .split(area);

    let form = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Pool Name:  ", Style::default().fg(SAGE)),
            Span::styled(
                if state.input_name.is_empty() { "(enter pool name)" } else { &state.input_name },
                if state.input_name.is_empty() { Style::default().fg(Color::DarkGray) } else { Style::default().fg(CREAM).add_modifier(Modifier::BOLD) },
            ),
        ]),
        Line::from(vec![
            Span::styled("  Bond (V):   ", Style::default().fg(SAGE)),
            Span::styled(&state.input_bond, Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(Span::styled("  Cost: 5 VIRTUAL to create pool", Style::default().fg(CYAN))),
        Line::from(""),
        Line::from(Span::styled("  [Enter] Create pool", Style::default().fg(GOLD).add_modifier(Modifier::BOLD))),
    ])
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(BLUE)).title(" Create Pool "));
    f.render_widget(form, layout[0]);

    let info = Paragraph::new(vec![
        Line::from(Span::styled("  Pool economics:", Style::default().fg(SAGE).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("  • Pool creator earns 5% of all member G-score rewards", Style::default().fg(CREAM))),
        Line::from(Span::styled("  • Members bond VIRTUAL and earn proportional rewards", Style::default().fg(CREAM))),
        Line::from(Span::styled("  • Closed pool: creator can allow/deny join requests", Style::default().fg(CREAM))),
    ])
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(FOREST)).title(" Info "))
    .wrap(Wrap { trim: true });
    f.render_widget(info, layout[1]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pools_tab_cycle() {
        assert_eq!(PoolsTab::Browse.next(), PoolsTab::Contracts);
        assert_eq!(PoolsTab::Contracts.next(), PoolsTab::MyPools);
        assert_eq!(PoolsTab::MyPools.next(), PoolsTab::Create);
        assert_eq!(PoolsTab::Create.next(), PoolsTab::Browse);
    }

    #[test]
    fn pools_state_stub_data() {
        let state = crate::tui::state::PoolsState::new();
        assert!(!state.pools.is_empty());
        assert!(state.pools.iter().any(|p| p.status == "open"));
    }

    #[test]
    fn pools_selected_returns_first() {
        let state = crate::tui::state::PoolsState::new();
        assert!(state.selected().is_some());
    }
}
