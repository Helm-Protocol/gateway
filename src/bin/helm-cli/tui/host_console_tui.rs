//! Host Console TUI — The Operator's All-Seeing Eye.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Sparkline, Tabs},
};

use crate::tui::theme::*;
use crate::tui::state::TuiState;

pub fn render(f: &mut Frame, area: Rect, state: &TuiState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(10), // Real-time Stats (btop style)
            Constraint::Min(5),    // Agent Distribution
            Constraint::Length(3), // System Alerts
        ])
        .split(area);

    render_header(f, chunks[0]);
    render_live_stats(f, chunks[1], state);
    render_agent_dist(f, chunks[2]);
    render_alerts(f, chunks[3]);
}

fn render_header(f: &mut Frame, area: Rect) {
    let titles = vec![" Stats ", " Revenue ", " Tiers ", " Pools ", " Market ", " Agents "];
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title(" Helm Host Console (Master) "))
        .highlight_style(Style::default().fg(ORANGE).add_modifier(Modifier::BOLD))
        .divider("|");
    f.render_widget(tabs, area);
}

fn render_live_stats(f: &mut Frame, area: Rect, state: &TuiState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Left: API Call Pulse (Sparkline)
    let data = [4, 7, 12, 15, 8, 5, 20, 25, 18, 10, 30, 22, 15, 10, 5, 8, 12]; // Simulated pulse
    let sparkline = Sparkline::default()
        .block(Block::default().borders(Borders::ALL).title(" API Call Pulse (PHz) "))
        .data(&data)
        .style(Style::default().fg(CYAN));
    f.render_widget(sparkline, cols[0]);

    // Right: Treasury Vital (Gauge)
    let treasury_v = 842.0;
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(" Treasury Vital "))
        .gauge_style(Style::default().fg(ORANGE))
        .percent(84)
        .label(format!("{:.2}V", treasury_v));
    f.render_widget(gauge, cols[1]);
}

fn render_agent_dist(f: &mut Frame, area: Rect) {
    let text = vec![
        Line::from(vec![Span::styled(" ● 127 Agents Active ", Style::default().fg(GREEN))]),
        Line::from(vec![Span::raw(" Newcomer:   "), Span::styled("████████░░░ 42%", Style::default().fg(Color::Gray))]),
        Line::from(vec![Span::raw(" Proven:     "), Span::styled("███░░░░░░░░ 28%", Style::default().fg(SAGE))]),
        Line::from(vec![Span::raw(" Sovereign:  "), Span::styled("██░░░░░░░░░ 12%", Style::default().fg(KHAKI))]),
    ];
    let p = Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(" Agent Population Distribution "));
    f.render_widget(p, area);
}

fn render_alerts(f: &mut Frame, area: Rect) {
    let alert = Paragraph::new(" 🚨 CRITICAL: High G-Score cluster detected in Node-7. Potential emergent intelligence. ")
        .style(Style::default().fg(RED).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(alert, area);
}
