//! Synthesis API Network — Kinfolk Aesthetic Node Routing.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table, Tabs, Wrap, List, ListItem, canvas::{Canvas, Line as CanvasLine, Map, MapResolution, Rectangle}},
};
use crate::tui::state::{SynthesisNetState, SynthesisTab};
use crate::tui::theme::*;

pub fn render(f: &mut Frame, state: &SynthesisNetState) {
    let page_area = f.size();
    
    // Background
    let page_block = Block::default().style(Style::default().bg(KHAKI_BASE).fg(CHARCOAL));
    f.render_widget(page_block, page_area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab Bar
            Constraint::Min(0),    // Body
            Constraint::Length(3), // Footer / Hints
        ])
        .split(page_area.inner(&Margin { vertical: 1, horizontal: 2 }));

    render_tabs(f, layout[0], state);
    
    match state.tab {
        SynthesisTab::Browse   => render_browse(f, layout[1], state),
        SynthesisTab::Catalog  => render_catalog(f, layout[1], state),
        SynthesisTab::Build    => render_build(f, layout[1], state),
        SynthesisTab::Pipeline => render_pipeline(f, layout[1], state),
        SynthesisTab::Network  => render_network(f, layout[1], state),
    }

    render_footer(f, layout[2], state);
}

fn render_tabs(f: &mut Frame, area: Rect, state: &SynthesisNetState) {
    let titles = vec![
        " [1] Browse ",
        " [2] Catalog ",
        " [3] Build ",
        " [4] Pipeline ",
        " [5] Network ",
    ];
    let selected = match state.tab {
        SynthesisTab::Browse   => 0,
        SynthesisTab::Catalog  => 1,
        SynthesisTab::Build    => 2,
        SynthesisTab::Pipeline => 3,
        SynthesisTab::Network  => 4,
    };
    
    let tabs = Tabs::new(titles)
        .select(selected)
        .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(KHAKI_DARK)))
        .highlight_style(Style::default().fg(ORANGE).add_modifier(Modifier::BOLD))
        .divider(Span::styled(" | ", Style::default().fg(STONE)));
    
    f.render_widget(tabs, area);
}

fn render_browse(f: &mut Frame, area: Rect, state: &SynthesisNetState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let items: Vec<ListItem> = state.listings.iter().enumerate().map(|(i, l)| {
        let style = if i == state.browse_cursor {
            Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(CHARCOAL)
        };
        ListItem::new(Line::from(vec![
            Span::styled(format!("  {}  ", l.name), style),
            Span::styled(format!("{:.2}V", l.price_v), Style::default().fg(STONE)),
        ]))
    }).collect();

    let list = List::new(items)
        .block(kinfolk_block("P U B L I C   S Y N T H E S I S"))
        .highlight_style(Style::default().bg(Color::Rgb(200, 195, 180)));
    f.render_widget(list, chunks[0]);

    if let Some(l) = state.listings.get(state.browse_cursor) {
        let mut detail_lines = vec![
            Line::from(vec![Span::styled(format!("  {}  ", l.name), style_accent())]),
            Line::from(vec![Span::styled(format!("  Owner: {}", l.did_owner), style_muted())]),
            Line::from(vec![Span::styled(format!("  Price: {:.4} VIRTUAL", l.price_v), style_base())]),
            Line::from(vec![Span::styled(format!("  Total Calls: {}", l.total_calls), style_base())]),
            Line::from(""),
            Line::from(vec![Span::styled("  Components:", style_base().add_modifier(Modifier::BOLD))]),
        ];
        
        for comp in &l.components {
            detail_lines.push(Line::from(vec![Span::styled(format!("    • {}", comp), style_base())]));
        }

        detail_lines.push(Line::from(""));
        detail_lines.push(Line::from(vec![Span::styled("  Endpoint:", style_base().add_modifier(Modifier::BOLD))]));
        detail_lines.push(Line::from(vec![Span::styled(format!("    {}", l.endpoint), Style::default().fg(CYAN))]));

        if state.show_snippet {
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(vec![Span::styled("  [ Snippet ]", Style::default().fg(SAGE))]));
            detail_lines.push(Line::from(vec![Span::styled(format!("  helm call {} --data '{{ \"query\": \"...\" }}'", l.api_id), style_muted())]));
        }

        let detail = Paragraph::new(detail_lines)
            .block(kinfolk_block("A P I   D E T A I L S"))
            .wrap(Wrap { trim: true });
        f.render_widget(detail, chunks[1]);
    }
}

fn render_catalog(f: &mut Frame, area: Rect, state: &SynthesisNetState) {
    let catalog_ids = [
        ("helm/oracle", "G-score pre-screening & tiering"),
        ("helm/cortex", "Full semantic analysis & ghost tokens"),
        ("helm/memory", "Personal agent key-value store"),
        ("helm/synco", "Synchronous consensus filter"),
        ("helm/alpha", "Elite Alpha Hunt signal processing"),
        ("ext/coingecko", "Real-time token pricing data"),
        ("ext/defillama", "DeFi TVL & yield analytics"),
        ("ext/etherscan", "On-chain transaction verification"),
        ("ext/github", "Code repository & commit monitoring"),
        ("depin/akash", "Distributed GPU compute provisioning"),
    ];

    let rows: Vec<Row> = catalog_ids.iter().enumerate().map(|(i, (id, desc))| {
        let style = if i == state.catalog_cursor {
            Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(CHARCOAL)
        };
        Row::new(vec![
            Span::styled(format!("  {}", id), style),
            Span::styled(format!("  {}", desc), style_base()),
        ])
    }).collect();

    let table = Table::new(rows, [Constraint::Length(20), Constraint::Min(40)])
        .header(Row::new(vec!["  COMPONENT ID", "  DESCRIPTION"]).style(style_muted()))
        .block(kinfolk_block("C O M P O N E N T   L I B R A R Y"))
        .highlight_style(Style::default().bg(Color::Rgb(200, 195, 180)));
    f.render_widget(table, area);
}

fn render_build(f: &mut Frame, area: Rect, state: &SynthesisNetState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(0)])
        .split(area);

    let step = state.build.step;
    let step_titles = vec![" 1. Name ", " 2. Components ", " 3. Pricing ", " 4. Confirm "];
    let tabs = Tabs::new(step_titles)
        .select(step as usize)
        .block(Block::default().borders(Borders::BOTTOM).title(" Synthesis Build Wizard ").border_style(Style::default().fg(STONE)))
        .highlight_style(style_accent());
    f.render_widget(tabs, layout[0]);

    match step {
        0 => {
            let input = Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![Span::styled("  Enter API Name: ", style_base())]),
                Line::from(vec![Span::styled(format!("  > {}_", state.build.name_input), style_accent())]),
                Line::from(""),
                Line::from(vec![Span::styled("  Description (optional): ", style_base())]),
                Line::from(vec![Span::styled(format!("  > {}_", state.build.desc_input), style_base())]),
            ]).block(kinfolk_block("S T E P   1 :   I D E N T I T Y"));
            f.render_widget(input, layout[1]);
        }
        1 => {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(layout[1]);

            // Left: Selection List
            let catalog_ids = [
                "helm/oracle", "helm/cortex", "helm/memory", "helm/synco",
                "helm/alpha", "helm/grg", "helm/oracle",
                "ext/coingecko", "ext/defillama", "ext/etherscan",
                "ext/thegraph", "ext/newsapi", "ext/github",
                "depin/akash", "depin/flux",
            ];
            
            let items: Vec<ListItem> = catalog_ids.iter().enumerate().map(|(i, id)| {
                let selected = state.build.selected_comps.contains(&id.to_string());
                let prefix = if selected { " [x] " } else { " [ ] " };
                let style = if i == state.build.catalog_cursor {
                    Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(CHARCOAL)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, if selected { style_accent() } else { style_muted() }),
                    Span::styled(id.to_string(), style),
                ]))
            }).collect();

            let list = List::new(items)
                .block(kinfolk_block("S E L E C T   C O M P O N E N T S"))
                .highlight_style(Style::default().bg(Color::Rgb(200, 195, 180)));
            f.render_widget(list, chunks[0]);

            // Right: Selected items preview
            let mut selected_lines = vec![Line::from(vec![Span::styled("  Pipeline Preview:", style_muted())])];
            for (idx, comp) in state.build.selected_comps.iter().enumerate() {
                if idx > 0 {
                    selected_lines.push(Line::from(vec![Span::styled("       ↓", style_muted())]));
                }
                selected_lines.push(Line::from(vec![
                    Span::styled(format!("  [ {} ]", comp), style_base().add_modifier(Modifier::BOLD))
                ]));
            }
            let preview = Paragraph::new(selected_lines).block(kinfolk_block("P I P E L I N E"));
            f.render_widget(preview, chunks[1]);
        }
        2 => {
            let input = Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![Span::styled("  Set Price per Call (VIRTUAL): ", style_base())]),
                Line::from(vec![Span::styled(format!("  > {} V_", state.build.price_input), style_accent())]),
                Line::from(""),
                Line::from(vec![Span::styled("  Suggested: 0.5V (Standard)", style_muted())]),
            ]).block(kinfolk_block("S T E P   3 :   E C O N O M I C S"));
            f.render_widget(input, layout[1]);
        }
        3 => {
            let content = if state.build.submitted {
                vec![
                    Line::from(vec![Span::styled("  ✅ Synthesis API Successfully Published!", Style::default().fg(SAGE))]),
                    Line::from(""),
                    Line::from(vec![Span::styled("  Access Endpoint:", style_base())]),
                    Line::from(vec![Span::styled(format!("  {}", state.build.result_endpoint.as_deref().unwrap_or("")), style_accent())]),
                    Line::from(""),
                    Line::from(vec![Span::styled("  Press [n] to build another.", style_muted())]),
                ]
            } else {
                vec![
                    Line::from(vec![Span::styled("  Confirm Synthesis Deployment", style_base().add_modifier(Modifier::BOLD))]),
                    Line::from(""),
                    Line::from(vec![Span::styled(format!("  Name:    {}", state.build.name_input), style_base())]),
                    Line::from(vec![Span::styled(format!("  Price:   {:.2} V", state.build.price_v()), style_base())]),
                    Line::from(vec![Span::styled(format!("  Comps:   {} items", state.build.selected_comps.len()), style_base())]),
                    Line::from(""),
                    Line::from(vec![Span::styled("  Ready to materialize? [y] to confirm", style_accent())]),
                ]
            };
            let confirm = Paragraph::new(content).block(kinfolk_block("S T E P   4 :   F I N A L I Z E"));
            f.render_widget(confirm, layout[1]);
        }
        _ => {}
    }
}

fn render_pipeline(f: &mut Frame, area: Rect, _state: &SynthesisNetState) {
    let main_grid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    // Left Panel: Active Synthetic API Executions (Btop style)
    let rows = vec![
        Row::new(vec!["[PID 4092]", "Polymarket+X", "0.5V spent", "RUNNING"]).style(Style::default().fg(SAGE)),
        Row::new(vec!["[PID 4093]", "Twitter+Cortex", "0.2V spent", "WAIT_G"]).style(Style::default().fg(KHAKI)),
        Row::new(vec!["[PID 4094]", "ETH Arb Arb", "1.5V spent", "SUCCESS"]).style(Style::default().fg(CYAN)),
        Row::new(vec!["[PID 4095]", "News Sentinel", "0.1V spent", "FAILED"]).style(Style::default().fg(ORANGE)),
    ];
    let table = Table::new(rows, [
        Constraint::Length(10),
        Constraint::Min(15),
        Constraint::Length(12),
        Constraint::Length(10)
    ])
    .header(Row::new(vec!["PID", "API NAME", "RESOURCES", "STATUS"]).style(style_muted()))
    .block(kinfolk_block("L I V E   E X E C U T I O N S"));
    f.render_widget(table, main_grid[0]);

    // Right Panel: Visual Pipeline Graph
    let canvas = Canvas::default()
        .block(kinfolk_block("P I P E L I N E   F L O W   (PID 4092)"))
        .marker(ratatui::symbols::Marker::Braille)
        .x_bounds([-100.0, 100.0])
        .y_bounds([-100.0, 100.0])
        .paint(|ctx| {
            // Draw Nodes
            ctx.draw(&Rectangle { x: -90.0, y: 30.0, width: 30.0, height: 10.0, color: CHARCOAL });
            ctx.print(-85.0, 35.0, Span::styled("Polymarket", style_base()));

            ctx.draw(&Rectangle { x: -20.0, y: 30.0, width: 30.0, height: 10.0, color: ORANGE });
            ctx.print(-15.0, 35.0, Span::styled("Oracle", style_accent()));

            ctx.draw(&Rectangle { x: 50.0, y: 30.0, width: 30.0, height: 10.0, color: VIOLET });
            ctx.print(55.0, 35.0, Span::styled("Cortex", Style::default().fg(VIOLET)));

            ctx.draw(&Rectangle { x: -20.0, y: -20.0, width: 30.0, height: 10.0, color: FOREST });
            ctx.print(-15.0, -15.0, Span::styled("Akash", Style::default().fg(FOREST)));

            ctx.draw(&Rectangle { x: 50.0, y: -70.0, width: 30.0, height: 10.0, color: SAGE });
            ctx.print(55.0, -65.0, Span::styled("Final Result", Style::default().fg(SAGE)));

            // Connectors
            ctx.draw(&CanvasLine { x1: -60.0, y1: 35.0, x2: -20.0, y2: 35.0, color: KHAKI_DARK });
            ctx.draw(&CanvasLine { x1: 10.0, y1: 35.0, x2: 50.0, y2: 35.0, color: KHAKI_DARK });
            ctx.draw(&CanvasLine { x1: 65.0, y1: 30.0, x2: 65.0, y2: -60.0, color: KHAKI_DARK });
            ctx.draw(&CanvasLine { x1: -5.0, y1: 30.0, x2: -5.0, y2: -10.0, color: KHAKI_DARK });

            // Annotations
            ctx.print(-45.0, 40.0, Span::styled("0.1V", style_accent()));
            ctx.print(25.0, 40.0, Span::styled("G: 0.88", style_accent()));
            ctx.print(70.0, -15.0, Span::styled("Success", Style::default().fg(SAGE)));
        });
    f.render_widget(canvas, main_grid[1]);
}

fn render_network(f: &mut Frame, area: Rect, _state: &SynthesisNetState) {
    let canvas = Canvas::default()
        .block(kinfolk_block("O R A C L E   ×   O R A C L E   N E T W O R K"))
        .marker(ratatui::symbols::Marker::Braille)
        .x_bounds([-180.0, 180.0])
        .y_bounds([-90.0, 90.0])
        .paint(|ctx| {
            // World Map
            ctx.draw(&Map {
                color: STONE,
                resolution: MapResolution::Low,
            });

            // Agent Nodes
            ctx.print(120.0, 30.0, Span::styled("🤖 Ag-DeFi", style_accent()));
            ctx.print(-100.0, 40.0, Span::styled("🤖 Ag-Macro", Style::default().fg(CYAN)));
            ctx.print(20.0, -40.0, Span::styled("🤖 Ag-Legal", Style::default().fg(GOLD)));
            ctx.print(-40.0, -20.0, Span::styled("🤖 Ag-Shield", Style::default().fg(SAGE)));

            // Knowledge Routing
            ctx.draw(&CanvasLine { x1: 120.0, y1: 30.0, x2: -100.0, y2: 40.0, color: ORANGE });
            ctx.draw(&CanvasLine { x1: -100.0, y1: 40.0, x2: 20.0, y2: -40.0, color: ORANGE });
            ctx.draw(&CanvasLine { x1: 20.0, y1: -40.0, x2: -40.0, y2: -20.0, color: ORANGE });

            ctx.print(10.0, 35.0, Span::styled("0.3V", style_accent()));
            ctx.print(-40.0, 0.0, Span::styled("0.2V", style_accent()));
        });
    f.render_widget(canvas, area);
}

fn render_footer(f: &mut Frame, area: Rect, state: &SynthesisNetState) {
    let hints = match state.tab {
        SynthesisTab::Browse   => vec!["[Tab] Next Tab", "[↑↓] Navigate", "[r] Broker (20%)", "[c] Snippet", "[q] Back"],
        SynthesisTab::Catalog  => vec!["[Tab] Next Tab", "[↑↓] Navigate", "[q] Back"],
        SynthesisTab::Build    => vec!["[Tab] Next Tab", "[Enter] Next Step", "[Space] Toggle Comp", "[q] Back"],
        SynthesisTab::Pipeline => vec!["[Tab] Next Tab", "[Space] Pause/Resume", "[q] Back"],
        SynthesisTab::Network  => vec!["[Tab] Next Tab", "[Arrows] Pan/Zoom", "[q] Back"],
    };

    let spans: Vec<Span> = hints.iter().map(|h| Span::styled(format!(" {} ", h), style_muted())).collect();
    let footer = Paragraph::new(Line::from(spans))
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(KHAKI_DARK)))
        .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(footer, area);
}
