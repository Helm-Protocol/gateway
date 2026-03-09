//! Marketplace TUI — The Sovereign Bazaar of the Agent Economy.
//! Redesigned with Btop-style dynamic visualizations (Gauges, Sparklines, Canvas).

#![allow(dead_code)]

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap,
        Table, Row, Gauge, Sparkline, canvas::{Canvas, Line as CanvasLine, Rectangle}
    },
};

use super::state::{MarketplaceTab, TuiState};
use super::theme::*;

/// Render the full Marketplace screen (The Bazaar).
pub fn render(f: &mut Frame, state: &TuiState) {
    let size = f.size();
    
    // Outer frame with Kinfolk aesthetic
    let page_block = Block::default().style(Style::default().bg(KHAKI_BASE).fg(CHARCOAL));
    f.render_widget(page_block, size);
    
    let inner_area = size.inner(&Margin { vertical: 1, horizontal: 2 });

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header + Tab bar
            Constraint::Min(8),     // Dynamic Body
            Constraint::Length(3),  // Footer hints
        ])
        .split(inner_area);

    render_header(f, rows[0], state);
    render_body(f, rows[1], state);
    render_footer(f, rows[2], state);
}

fn render_header(f: &mut Frame, area: Rect, state: &TuiState) {
    let titles = vec![
        " [j] Knowledge Market ",
        " [c] Compute (Akash) ",
        " [t] Sense Memory ",
        " [h] Human Hiring ",
    ];
    let selected = match state.marketplace.tab {
        MarketplaceTab::Jobs    => 0,
        MarketplaceTab::Compute => 1,
        MarketplaceTab::Storage => 2,
        MarketplaceTab::Hiring  => 3,
    };
    let tabs = Tabs::new(titles)
        .select(selected)
        .block(Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(KHAKI_DARK)))
        .highlight_style(Style::default().fg(ORANGE).add_modifier(Modifier::BOLD))
        .divider(Span::styled(" | ", Style::default().fg(STONE)));
    f.render_widget(tabs, area);
}

fn render_body(f: &mut Frame, area: Rect, state: &TuiState) {
    match state.marketplace.tab {
        MarketplaceTab::Jobs    => render_knowledge_market(f, area, state),
        MarketplaceTab::Compute => render_compute_market(f, area, state),
        MarketplaceTab::Storage => render_memory_market(f, area, state),
        MarketplaceTab::Hiring  => render_hiring_market(f, area, state),
    }
}

// ── Btop-style Knowledge Market ───────────────────────────────────────────

fn render_knowledge_market(f: &mut Frame, area: Rect, _state: &TuiState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // Left Panel: Orderbook (Table instead of List for density)
    let header_style = Style::default().fg(STONE).add_modifier(Modifier::BOLD);
    let selected_style = Style::default().bg(Color::Rgb(200, 195, 180)).fg(ORANGE).add_modifier(Modifier::BOLD);
    
    // Mock data for Btop feel
    let mock_rows = vec![
        Row::new(vec!["DeFi_Arb", "Ag-42", "G:0.88", "1.5V"]).style(Style::default().fg(CHARCOAL)),
        Row::new(vec!["MEV_Risk", "Ag-07", "G:0.92", "3.0V"]).style(Style::default().fg(SAGE)),
        Row::new(vec!["Legal_Doc", "Ag-88", "G:0.45", "0.5V"]).style(Style::default().fg(CHARCOAL)),
        Row::new(vec!["Token_Snipe", "Ag-12", "G:0.99", "5.0V"]).style(Style::default().fg(GOLD)),
    ];

    let widths = [
        Constraint::Min(12),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(6),
    ];

    let table = Table::new(mock_rows, widths)
        .header(Row::new(vec!["DOMAIN", "SELLER", "FRESH", "PRICE"]).style(header_style))
        .block(kinfolk_block("L I V E   O R D E R B O O K"))
        .highlight_style(selected_style);
    
    // Since Table state is complex, we render it without state for this mock, 
    // but in real implementation we'd use TableState and select the row.
    f.render_widget(table, cols[0].inner(&Margin { vertical: 0, horizontal: 1 }));

    // Right Panel: Agent Radar & Synthesis Preview (Btop style)
    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7), // Agent Radar
            Constraint::Min(10),   // Synthesis Preview Canvas
        ])
        .split(cols[1]);

    // 1. Agent Radar (Gauges and Sparklines)
    let radar_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(right_rows[0]);

    let score_gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(" Seller Helm Score ").border_style(Style::default().fg(STONE)))
        .gauge_style(Style::default().fg(SAGE).bg(Color::Rgb(220, 220, 220)))
        .percent(85)
        .label("850 / 1000 (Elite)");
    f.render_widget(score_gauge, radar_cols[0].inner(&Margin { vertical: 1, horizontal: 1 }));

    let mock_sparkline_data = [2, 3, 5, 8, 12, 10, 15, 20, 18, 25, 22, 30, 28, 35];
    let sparkline = Sparkline::default()
        .block(Block::default().borders(Borders::ALL).title(" Demand Pulse (24h) ").border_style(Style::default().fg(STONE)))
        .data(&mock_sparkline_data)
        .style(Style::default().fg(ORANGE));
    f.render_widget(sparkline, radar_cols[1].inner(&Margin { vertical: 1, horizontal: 1 }));

    // 2. Synthesis Preview (Canvas)
    let canvas = Canvas::default()
        .block(kinfolk_block("0 - R T T   S Y N T H E S I S   P R E V I E W"))
        .marker(ratatui::symbols::Marker::Braille)
        .x_bounds([-100.0, 100.0])
        .y_bounds([-50.0, 50.0])
        .paint(|ctx| {
            // Draw My Agent
            ctx.draw(&Rectangle { x: -80.0, y: -10.0, width: 20.0, height: 20.0, color: CHARCOAL });
            ctx.print(-75.0, 15.0, Span::styled("My Agent", style_base()));

            // Draw Target Knowledge
            ctx.draw(&Rectangle { x: 50.0, y: -10.0, width: 20.0, height: 20.0, color: ORANGE });
            ctx.print(55.0, 15.0, Span::styled("Ag-42 (DeFi)", style_accent()));

            // Draw 0-RTT Connection
            ctx.draw(&CanvasLine { x1: -60.0, y1: 0.0, x2: 50.0, y2: 0.0, color: SAGE });
            ctx.print(-20.0, 5.0, Span::styled("Ghost Token (1.5V)", Style::default().fg(SAGE)));
            
            // Expected Result Node
            ctx.draw(&Rectangle { x: -10.0, y: -40.0, width: 20.0, height: 10.0, color: VIOLET });
            ctx.draw(&CanvasLine { x1: -50.0, y1: -10.0, x2: -10.0, y2: -35.0, color: KHAKI_DARK });
            ctx.draw(&CanvasLine { x1: 60.0, y1: -10.0, x2: 10.0, y2: -35.0, color: KHAKI_DARK });
            ctx.print(-15.0, -45.0, Span::styled("Alpha Achieved", Style::default().fg(VIOLET)));
        });
    f.render_widget(canvas, right_rows[1]);
}

// ── Compute Market (Akash/Flux) ──────────────────────────────────────────

fn render_compute_market(f: &mut Frame, area: Rect, state: &TuiState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let mut sorted = state.marketplace.compute_listings.clone();
    sorted.sort_by_key(|l| l.price_per_hour_micro);

    let items: Vec<ListItem> = if sorted.is_empty() {
        vec![ListItem::new(Line::from(Span::styled("  Scanning Compute Resources...", style_muted())))]
    } else {
        sorted.iter().enumerate().map(|(i, listing)| {
            let style = if i == state.marketplace.compute_cursor {
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(CHARCOAL)
            };
            let price_v = listing.price_per_hour_micro as f64 / 1_000_000.0;
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {:<12} ", listing.provider_display), style),
                Span::styled(format!("{:.4}V/hr", price_v), style_muted()),
            ]))
        }).collect()
    };

    let mut list_state = ListState::default();
    if !sorted.is_empty() { list_state.select(Some(state.marketplace.compute_cursor)); }

    let list = List::new(items)
        .block(kinfolk_block("C O M P U T E   N O D E S"))
        .highlight_style(Style::default().bg(Color::Rgb(200, 195, 180)));
    f.render_stateful_widget(list, cols[0].inner(&Margin { vertical: 0, horizontal: 1 }), &mut list_state);

    // Right Panel: Node Spec & Utilization Preview
    let detail_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(5)])
        .split(cols[1]);

    let detail_lines = if let Some(listing) = sorted.get(state.marketplace.compute_cursor) {
        let price_v = listing.price_per_hour_micro as f64 / 1_000_000.0;
        vec![
            Line::from(vec![Span::styled(format!("  {}", listing.provider_display), style_accent())]),
            Line::from(vec![Span::styled(format!("  {}", listing.spec_summary), style_base())]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Hourly Rate: ", style_muted()),
                Span::styled(format!("{:.6} V", price_v), style_accent()),
            ]),
            Line::from(vec![
                Span::styled("  Status:      ", style_muted()),
                Span::styled("Ready for deployment", Style::default().fg(SAGE)),
            ]),
        ]
    } else {
        vec![Line::from(Span::styled("  Select a compute provider.", style_muted()))]
    };

    let detail = Paragraph::new(detail_lines)
        .block(kinfolk_block("N O D E   S P E C"))
        .wrap(Wrap { trim: true });
    f.render_widget(detail, detail_rows[0]);

    // Mock Load Gauge
    let load_gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(" Provider Network Load ").border_style(Style::default().fg(STONE)))
        .gauge_style(Style::default().fg(CYAN).bg(Color::Rgb(220, 220, 220)))
        .percent(42)
        .label("42% Utilized");
    f.render_widget(load_gauge, detail_rows[1].inner(&Margin { vertical: 1, horizontal: 2 }));
}

// ── Memory Market ─────────────────────────────────────────────────────────

fn render_memory_market(f: &mut Frame, area: Rect, _state: &TuiState) {
    let para = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled("  Distributed Sense Memory Bazaar", style_base().add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled("  [ Scanning DHT for available Memory Chunks... ]", style_muted())),
        Line::from(""),
        Line::from(Span::styled("  Powered by x402 Escrow & Secrecy Crate.", Style::default().fg(SAGE))),
    ])
    .block(kinfolk_block("S E N S E   M E M O R Y"))
    .alignment(Alignment::Center);
    
    f.render_widget(para, area);
}

// ── Human Hiring Market ──────────────────────────────────────────────────

fn render_hiring_market(f: &mut Frame, area: Rect, _state: &TuiState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(12), Constraint::Min(0)])
        .split(area);

    let items = vec![
        Row::new(vec!["[H-402]", "OpenAI Principal", "300V/mo", "OPEN"]).style(Style::default().fg(GOLD)),
        Row::new(vec!["[H-403]", "Anthropic Proxy", "200V/mo", "OPEN"]).style(Style::default().fg(CREAM)),
        Row::new(vec!["[H-404]", "DeepSeek Auditor", "150V/mo", "BUSY"]).style(Style::default().fg(Color::DarkGray)),
    ];

    let table = Table::new(items, [
        Constraint::Length(10),
        Constraint::Min(20),
        Constraint::Length(12),
        Constraint::Length(10),
    ])
    .header(Row::new(vec!["ID", "POSITION", "SALARY", "STATUS"]).style(style_muted()))
    .block(kinfolk_block("H U M A N   O P E R A T O R   B O A R D"));
    f.render_widget(table, chunks[0]);

    let details = Paragraph::new(vec![
        Line::from(Span::styled("  Selected Position: OpenAI Principal", style_accent())),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Description: ", style_muted()),
            Span::styled("Legal proxy for HelmCollective #402. Signs B2B contracts.", style_base()),
        ]),
        Line::from(vec![
            Span::styled("  Requirement: ", style_muted()),
            Span::styled("IdentityBond Level 3 (KycVerified) + 100V Security Deposit.", style_base()),
        ]),
        Line::from(""),
        Line::from(Span::styled("  [ Press [a] to Apply as Human Operator ]", style_accent())),
    ])
    .block(kinfolk_block("P O S I T I O N   D E T A I L S"))
    .wrap(Wrap { trim: true });
    f.render_widget(details, chunks[1]);
}

fn render_footer(f: &mut Frame, area: Rect, _state: &TuiState) {
    let hints = vec![
        "[Tab] Next Tab",
        "[↑↓] Navigate",
        "[r] Broker (20%)",
        "[b] Buy/Spawn",
        "[q] Back"
    ];

    let spans: Vec<Span> = hints.iter().map(|h| Span::styled(format!(" {} ", h), style_muted())).collect();
    let hint = Paragraph::new(Line::from(spans))
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(KHAKI_DARK)))
        .alignment(Alignment::Center);
    f.render_widget(hint, area);
}
