//! Compute — DePIN GPU Marketplace TUI screen.
//!
//! ## Screen Layout
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────────┐
//! │  ⚡ COMPUTE — DePIN GPU Marketplace                    [Browse|Spawn]  │
//! ├────────────────────────────────────────────────────────────────────────┤
//! │  Provider  GPU              VRAM    vCPU   RAM   Price     Region      │
//! │  ──────────────────────────────────────────────────────────────────── │
//! │  ▶ Akash   A100 SXM4 80GB  80GB    30c    480G  4.50V/hr  US-East ●  │
//! │    Akash   H100 NVL 80GB   80GB    30c    480G  7.00V/hr  EU-West ●  │
//! │    Akash   RTX 4090 24GB   24GB    16c    128G  1.60V/hr  US-West ●  │
//! │    Flux    A40 48GB        48GB    24c    256G  3.20V/hr  Global  ●  │
//! │    Flux    CPU (4c/32G)    --       4c     32G  0.53V/hr  Global  ●  │
//! │    Render  RTX 3090 24GB   24GB     8c     64G  1.20V/hr  US-West ●  │
//! ├────────────────────────────────────────────────────────────────────────┤
//! │  [Enter] Spawn  [↑/↓] Select  [s] Sort by price  [q] Back            │
//! └────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Spawn Confirm Dialog
//!
//! ```text
//! ┌──────────────────────────────────────────────────┐
//! │  Spawn Confirmation                              │
//! │  Provider: Akash — A100 SXM4 80GB               │
//! │  Duration: [1___] hours                         │
//! │  Est. Cost: 4.50 VIRTUAL                        │
//! │  [y] Confirm  |  [Esc] Cancel                   │
//! └──────────────────────────────────────────────────┘
//! ```

#![allow(dead_code)]

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::tui::state::{ComputeState, TuiState};

const GOLD:   Color = Color::Rgb(212, 175, 55);
const FOREST: Color = Color::Rgb(34,  85,  34);
const SAGE:   Color = Color::Rgb(143, 188, 143);
const CREAM:  Color = Color::Rgb(255, 253, 208);
const CYAN:   Color = Color::Rgb(0,   200, 200);
const RED:    Color = Color::Rgb(220,  50,  47);
const GREEN:  Color = Color::Rgb(0,   200,  80);
const ORANGE: Color = Color::Rgb(215, 100,   0);

pub fn render(f: &mut Frame, tui: &TuiState) {
    let area = f.size();
    let state = &tui.compute;

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CYAN))
        .title(Span::styled(
            " ⚡ COMPUTE — DePIN GPU Marketplace ",
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center);
    f.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // column header
            Constraint::Min(4),    // provider list
            Constraint::Length(2), // help bar
        ])
        .margin(1)
        .split(area);

    // Column header
    let header = Paragraph::new(Line::from(vec![
        Span::styled(format!("  {:<10}", "Provider"), Style::default().fg(SAGE).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:<22}", "GPU"),        Style::default().fg(SAGE).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>6}  ", "VRAM"),      Style::default().fg(SAGE).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>5}  ", "vCPU"),      Style::default().fg(SAGE).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>5}  ", "RAM"),        Style::default().fg(SAGE).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{:>10}  ", "Price"),    Style::default().fg(SAGE).add_modifier(Modifier::BOLD)),
        Span::styled("Region", Style::default().fg(SAGE).add_modifier(Modifier::BOLD)),
    ]));
    f.render_widget(header, chunks[0]);

    // Provider list
    let items: Vec<ListItem> = state.providers.iter().enumerate().map(|(i, p)| {
        let selected = i == state.cursor;
        let prefix = if selected { "▶ " } else { "  " };
        let avail_dot = if p.available { Span::styled("●", Style::default().fg(GREEN)) }
                        else           { Span::styled("○", Style::default().fg(RED))   };
        let vram_str = if p.vram_gb == 0 { "   --".into() } else { format!("{:>4}GB", p.vram_gb) };
        let style = if selected {
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD)
        } else if p.available {
            Style::default().fg(CREAM)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        ListItem::new(Line::from(vec![
            Span::styled(format!("{}{:<10}", prefix, p.provider), style),
            Span::styled(format!("{:<22}", p.gpu),  style),
            Span::styled(format!("{} ", vram_str),  style),
            Span::styled(format!("{:>4}c  ", p.vcpu), style),
            Span::styled(format!("{:>4}G  ", p.ram_gb), style),
            Span::styled(format!("{:>7.2}V/hr  ", p.price_v_hr),
                if selected { Style::default().fg(GOLD).add_modifier(Modifier::BOLD) } else { Style::default().fg(CYAN) }),
            Span::styled(format!("{:<10}", p.region), style),
            avail_dot,
        ]))
    }).collect();

    let mut list_state = ListState::default();
    list_state.select(Some(state.cursor));
    let list = List::new(items)
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(FOREST)))
        .highlight_style(Style::default().fg(GOLD).add_modifier(Modifier::BOLD));
    f.render_stateful_widget(list, chunks[1], &mut list_state);

    // Help bar
    let help = if state.confirming {
        "[+/-] Hours  [y] Confirm spawn  [Esc] Cancel"
    } else {
        "[Enter] Spawn  [↑/↓] Select  [s] Sort by price  [q] Back"
    };
    f.render_widget(
        Paragraph::new(help).style(Style::default().fg(SAGE)).alignment(Alignment::Center),
        chunks[2],
    );

    // Spawn confirmation overlay
    if state.confirming {
        render_spawn_confirm(f, area, state);
    }
}

fn render_spawn_confirm(f: &mut Frame, area: Rect, state: &ComputeState) {
    let Some(p) = state.selected() else { return };
    if !p.available { return; }

    let popup_w = 56u16;
    let popup_h = 10u16;
    let x = area.x + area.width.saturating_sub(popup_w) / 2;
    let y = area.y + area.height.saturating_sub(popup_h) / 2;
    let popup = Rect { x, y, width: popup_w.min(area.width), height: popup_h.min(area.height) };

    f.render_widget(Clear, popup);

    let hours: f64 = state.spawn_hours_input.parse::<f64>().unwrap_or(1.0).max(0.1);
    let cost  = p.price_v_hr * hours;

    let body = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Provider: ", Style::default().fg(SAGE)),
            Span::styled(format!("{} — {}", p.provider, p.gpu),
                Style::default().fg(CREAM).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Region:   ", Style::default().fg(SAGE)),
            Span::styled(p.region, Style::default().fg(CYAN)),
        ]),
        Line::from(vec![
            Span::styled("  Duration: ", Style::default().fg(SAGE)),
            Span::styled(format!("[{}] hrs  (+/- to adjust)", state.spawn_hours_input),
                Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Est. Cost:", Style::default().fg(SAGE)),
            Span::styled(format!(" {:.2} VIRTUAL", cost),
                Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  [y] Confirm spawn  |  [Esc] Cancel",
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD),
        )),
    ])
    .block(Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ORANGE))
        .title(Span::styled(" Spawn Agent ", Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)))
    )
    .wrap(Wrap { trim: true });
    f.render_widget(body, popup);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn market_catalog_prices_are_market_rate() {
        let catalog = crate::tui::state::ComputeProviderView::market_catalog();
        for p in &catalog {
            // All GPU prices should be < 10 VIRTUAL/hr (market rate with 15% markup)
            assert!(p.price_v_hr < 10.0, "price too high for {}: {}V/hr", p.gpu, p.price_v_hr);
            // All GPU prices should be > 0
            assert!(p.price_v_hr > 0.0, "price is zero for {}", p.gpu);
        }
    }

    #[test]
    fn estimated_cost_calculation() {
        let mut state = crate::tui::state::ComputeState::new();
        state.spawn_hours_input = "2".into();
        // First provider is A100 at 4.5V/hr × 2hr = 9.0
        assert!((state.estimated_cost() - 9.0).abs() < 0.01);
    }

    #[test]
    fn compute_state_cursor_bounds() {
        let mut state = crate::tui::state::ComputeState::new();
        let len = state.providers.len();
        state.cursor = len + 100; // out of bounds
        assert!(state.selected().is_none()); // safe — returns None
    }
}
