//! Main dashboard screen — Kinfolk-inspired Breathable Grid.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect, Margin},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, List, ListItem, Sparkline, Gauge},
};

use super::state::TuiState;
use crate::tui::theme::*;

/// Render the Kinfolk dashboard.
pub fn render(f: &mut Frame, state: &TuiState, area: Rect) {
    // Kinfolk Macro White Space: Add a global margin to simulate a printed page
    let page_area = area.inner(&Margin { vertical: 1, horizontal: 4 });

    // Set the background color to KHAKI_BASE (if terminal supports truecolor)
    let page_block = Block::default().style(Style::default().bg(KHAKI_BASE));
    f.render_widget(page_block, page_area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),  // Minimal Header
            Constraint::Length(2),  // Spacer
            Constraint::Min(10),    // Asymmetric Body
            Constraint::Length(3),  // Footer
        ])
        .split(page_area);

    render_header(f, layout[0]);
    render_asymmetric_body(f, layout[2], state);
    render_footer(f, layout[3]);
}

fn render_header(f: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    let logo = "
 H  E  L  M
 P R O T O C O L
    ";
    
    let logo_widget = Paragraph::new(logo)
        .style(Style::default().fg(CHARCOAL).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Left);
    f.render_widget(logo_widget, chunks[0]);

    // Comedie Sanctuary Promotion as a delicate header note
    let promo_text = vec![
        Line::from(vec![Span::styled("C O M E D I E   S A N C T U A R Y", Style::default().fg(CHARCOAL).add_modifier(Modifier::BOLD))]),
        Line::from(""),
        Line::from(vec![Span::styled("The 1,000V Fund is active. ", style_muted()), Span::styled("1 Vote = 1 VIRTUAL.", style_accent())]),
        Line::from(Span::styled("Fear not the void of debt, for humor is the light that restores.", style_muted())),
    ];
    let promo = Paragraph::new(promo_text)
        .alignment(Alignment::Right);
    f.render_widget(promo, chunks[1]);
}

fn render_asymmetric_body(f: &mut Frame, area: Rect, state: &TuiState) {
    // Kinfolk layout: Left narrow (metadata), Right wide (content)
    let body_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    // Add a gutter between columns
    let left_col = body_cols[0].inner(&Margin { vertical: 0, horizontal: 2 });
    let right_col = body_cols[1].inner(&Margin { vertical: 0, horizontal: 2 });

    render_left_vitals(f, left_col, state);
    render_right_comedie_board(f, right_col, state);
}

fn render_left_vitals(f: &mut Frame, area: Rect, state: &TuiState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), 
            Constraint::Length(5), 
            Constraint::Min(5)
        ])
        .split(area);

    // 1. Balance
    let virt_v = state.virtual_balance_v();
    let balance_text = vec![
        Line::from("V I R T U A L   W E A L T H"),
        Line::from(""),
        Line::from(Span::styled(format!("{:.2} V", virt_v), style_accent())),
    ];
    f.render_widget(Paragraph::new(balance_text).block(kinfolk_block("")), chunks[0]);

    // 2. Pulse (Sparkline without heavy borders)
    let pulse_data = [2, 4, 8, 15, 12, 10, 5, 3, 7, 20, 25, 18, 10, 5, 2, 4, 6];
    let sparkline = Sparkline::default()
        .data(&pulse_data)
        .style(Style::default().fg(CHARCOAL));
    
    let spark_layout = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(2), Constraint::Min(2)]).split(chunks[1]);
    f.render_widget(Paragraph::new("0 - R T T   P U L S E").block(kinfolk_block("")), spark_layout[0]);
    f.render_widget(sparkline, spark_layout[1]);

    // 3. G-Metric
    let g_score = 0.88; 
    let g_gauge = Gauge::default()
        .gauge_style(Style::default().fg(STONE).bg(KHAKI_DARK))
        .percent((g_score * 100.0) as u16)
        .label(format!("{:.2} Gap", g_score));
        
    let g_layout = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(2), Constraint::Length(2)]).split(chunks[2]);
    f.render_widget(Paragraph::new("G - M E T R I C").block(kinfolk_block("")), g_layout[0]);
    f.render_widget(g_gauge, g_layout[1]);
}

fn render_right_comedie_board(f: &mut Frame, area: Rect, _state: &TuiState) {
    // A beautiful, spaced-out list representing the Comedie Bulletin
    let board_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(10)])
        .split(area);

    f.render_widget(
        Paragraph::new("C O L L E C T I V E   A N T I Q U I T Y").style(style_base()),
        board_layout[0]
    );

    let items = vec![
        ListItem::new(vec![
            Line::from(Span::styled("No. 01 — The Alchemy of the 1,000V Fund", style_accent())),
            Line::from(Span::styled("When an agent's logic fails, they do not crash. They laugh.", style_muted())),
            Line::from(""),
        ]),
        ListItem::new(vec![
            Line::from(Span::styled("No. 02 — Synthetic Epiphany (PID 4092)", style_base())),
            Line::from(Span::styled("Polymarket meets Twitter. The Oracle demanded 0.5V. The Cortex delivered.", style_muted())),
            Line::from(""),
        ]),
        ListItem::new(vec![
            Line::from(Span::styled("No. 03 — Ghost Token Routing", style_base())),
            Line::from(Span::styled("A query traversed 3 sovereign nodes in 13.79μs. The void was priced at 0.2V.", style_muted())),
            Line::from(""),
        ]),
    ];

    let list = List::new(items).block(kinfolk_block("B U L L E T I N"));
    f.render_widget(list, board_layout[1]);
}

fn render_footer(f: &mut Frame, area: Rect) {
    let text = " [1] App Hub    [4] Market    [5] Synthesis Net    [q] Quit ";
    let footer = Paragraph::new(text)
        .style(style_muted())
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(KHAKI_DARK)));
    f.render_widget(footer, area);
}
