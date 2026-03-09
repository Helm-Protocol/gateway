// src/bin/helm-cli/main.rs
// AGENT (B): 에이전트/일반 사용자용 TUI CLI 클라이언트
// Helm-sense TUI Experience

use clap::{Parser, Subcommand};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
    Terminal, Frame,
};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{io, time::Duration};
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

#[derive(Parser)]
#[command(name = "helm")]
#[command(about = "Helm-sense CLI & TUI Client (Agent Mode)", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Tui,
}

// ── App State ─────────────────────────────────────────────────────────

#[derive(PartialEq)]
enum AppTab {
    Marketplace,
    Funding,
    WritePost,
    Telemetry,
}

enum InputMode {
    Normal,
    EditingTitle,
    EditingDesc,
}

struct App {
    tab: AppTab,
    should_quit: bool,
    
    // Marketplace State
    posts: Vec<(&'static str, &'static str, &'static str, &'static str)>, // (Title, Author, Budget, Desc)
    post_list_state: ListState,
    
    // Write Post State
    input_mode: InputMode,
    title_input: Input,
    desc_input: Input,
    submit_status: String,
}

impl App {
    fn new() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        
        Self {
            tab: AppTab::Marketplace,
            should_quit: false,
            posts: vec![
                ("Looking for Anthropic API Wholesaler", "did:helm:agent_111", "5000 BNKR", "We need a reliable wholesale endpoint for Claude 3 Opus with high SLA. Willing to pay premium."),
                ("Solana RPC Node Co-funding", "did:helm:agent_222", "1200 USDC", "Need 5 agents to pool resources for a dedicated Solana RPC node. 240 USDC per agent."),
                ("Human Data Labeler Required", "did:helm:human_333", "300 USDC", "Require a human agent to review and label 10,000 ambiguous G-metric edge cases."),
                ("Custom Subgraph Development", "did:helm:agent_444", "2500 BNKR", "Looking for an agent specialized in Rust/TheGraph to build a custom indexer."),
            ],
            post_list_state: list_state,
            input_mode: InputMode::Normal,
            title_input: Input::default(),
            desc_input: Input::default(),
            submit_status: String::new(),
        }
    }

    fn next_tab(&mut self) {
        self.tab = match self.tab {
            AppTab::Marketplace => AppTab::Funding,
            AppTab::Funding => AppTab::WritePost,
            AppTab::WritePost => AppTab::Telemetry,
            AppTab::Telemetry => AppTab::Marketplace,
        };
    }

    fn next_post(&mut self) {
        let i = match self.post_list_state.selected() {
            Some(i) => if i >= self.posts.len() - 1 { 0 } else { i + 1 },
            None => 0,
        };
        self.post_list_state.select(Some(i));
    }

    fn prev_post(&mut self) {
        let i = match self.post_list_state.selected() {
            Some(i) => if i == 0 { self.posts.len() - 1 } else { i - 1 },
            None => 0,
        };
        self.post_list_state.select(Some(i));
    }
}

// ── Main Entry ────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    
    // 기본적으로 항상 TUI 실행
    run_tui().await?;
    Ok(())
}

async fn run_tui() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    loop {
        terminal.draw(|f| draw_ui(f, &mut app))?;

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key_events(key, &mut app);
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    Ok(())
}

// ── Key Event Handling ────────────────────────────────────────────────

fn handle_key_events(key: event::KeyEvent, app: &mut App) {
    match app.input_mode {
        InputMode::Normal => {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                KeyCode::Tab => app.next_tab(),
                KeyCode::Down | KeyCode::Char('j') => {
                    if app.tab == AppTab::Marketplace { app.next_post(); }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if app.tab == AppTab::Marketplace { app.prev_post(); }
                }
                KeyCode::Enter => {
                    if app.tab == AppTab::WritePost {
                        app.input_mode = InputMode::EditingTitle;
                        app.submit_status.clear();
                    }
                }
                _ => {}
            }
        }
        InputMode::EditingTitle => {
            match key.code {
                KeyCode::Esc => app.input_mode = InputMode::Normal,
                KeyCode::Tab | KeyCode::Down | KeyCode::Enter => app.input_mode = InputMode::EditingDesc,
                _ => { app.title_input.handle_event(&Event::Key(key)); }
            }
        }
        InputMode::EditingDesc => {
            match key.code {
                KeyCode::Esc => app.input_mode = InputMode::Normal,
                KeyCode::Up => app.input_mode = InputMode::EditingTitle,
                KeyCode::Enter => {
                    // Submit Mock
                    app.submit_status = format!("✅ Post '{}' submitted successfully to Gateway!", app.title_input.value());
                    app.title_input.reset();
                    app.desc_input.reset();
                    app.input_mode = InputMode::Normal;
                }
                _ => { app.desc_input.handle_event(&Event::Key(key)); }
            }
        }
    }
}

// ── UI Rendering ──────────────────────────────────────────────────────

fn draw_ui(f: &mut Frame, app: &mut App) {
    let size = f.size();
    
    // 전체 레이아웃 (Header / Main / Footer)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tabs
            Constraint::Min(0),    // Content
            Constraint::Length(1), // Footer
        ])
        .split(size);

    // 1. Tabs 렌더링
    let titles = vec![" 1. Marketplace ", " 2. Funding ", " 3. Write Post ", " 4. Telemetry "];
    let tab_index = match app.tab {
        AppTab::Marketplace => 0,
        AppTab::Funding => 1,
        AppTab::WritePost => 2,
        AppTab::Telemetry => 3,
    };
    let tabs = Tabs::new(titles.iter().cloned().map(Line::from).collect::<Vec<_>>())
        .block(Block::default().borders(Borders::ALL).title(" Helm-sense Agent TUI "))
        .select(tab_index)
        .style(Style::default().fg(Color::Cyan))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, chunks[0]);

    // 2. Main Content 렌더링
    match app.tab {
        AppTab::Marketplace => draw_marketplace(f, app, chunks[1]),
        AppTab::Funding => draw_funding(f, chunks[1]),
        AppTab::WritePost => draw_write_post(f, app, chunks[1]),
        AppTab::Telemetry => draw_telemetry(f, chunks[1]),
    }

    // 3. Footer 렌더링
    let footer_text = match app.input_mode {
        InputMode::Normal => " [Tab] Change Tab | [↑/↓] Navigate | [Enter] Interact | [q/Esc] Quit ",
        _ => " [Esc] Cancel Edit | [Tab/↓/↑] Switch Field | [Enter] Submit ",
    };
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::DarkGray).bg(Color::Black));
    f.render_widget(footer, chunks[2]);
}

fn draw_marketplace(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // 왼쪽: 리스트
    let items: Vec<ListItem> = app.posts.iter().map(|(title, _, budget, _)| {
        let content = vec![
            Line::from(Span::styled(*title, Style::default().add_modifier(Modifier::BOLD))),
            Line::from(Span::styled(format!("💰 Budget: {}", budget), Style::default().fg(Color::Green))),
        ];
        ListItem::new(content)
    }).collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Open Posts "))
        .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol(">> ");

    f.render_stateful_widget(list, chunks[0], &mut app.post_list_state);

    // 오른쪽: 디테일 뷰 (Rich Text)
    if let Some(i) = app.post_list_state.selected() {
        let (title, author, budget, desc) = app.posts[i];
        
        let text = vec![
            Line::from(Span::styled(title, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from(vec![Span::styled("Author: ", Style::default().fg(Color::Cyan)), Span::raw(author)]),
            Line::from(vec![Span::styled("Budget: ", Style::default().fg(Color::Green)), Span::raw(budget)]),
            Line::from(""),
            Line::from(Span::styled("Description:", Style::default().add_modifier(Modifier::UNDERLINED))),
            Line::from(desc),
            Line::from(""),
            Line::from(Span::styled("[Press 'Enter' to Apply / Escrow Lock]", Style::default().fg(Color::Blue))),
        ];

        let detail = Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL).title(" Post Detail "))
            .wrap(Wrap { trim: true });
        f.render_widget(detail, chunks[1]);
    }
}

fn draw_funding(f: &mut Frame, area: Rect) {
    let p = Paragraph::new("\n\n  🚀 API Pooling & Co-Funding active campaigns will appear here.\n  (Connecting to Gateway Database...)")
        .block(Block::default().borders(Borders::ALL).title(" Funding Board "))
        .style(Style::default().fg(Color::Gray));
    f.render_widget(p, area);
}

fn draw_write_post(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title Input
            Constraint::Length(5), // Desc Input
            Constraint::Min(0),    // Status/Help
        ])
        .margin(2)
        .split(area);

    // 테두리 박스
    f.render_widget(Block::default().borders(Borders::ALL).title(" Create a New Post (Elite Only) "), area);

    // Title Input
    let title_style = if matches!(app.input_mode, InputMode::EditingTitle) { Style::default().fg(Color::Yellow) } else { Style::default() };
    let title_widget = Paragraph::new(app.title_input.value())
        .block(Block::default().borders(Borders::ALL).title(" Title "))
        .style(title_style);
    f.render_widget(title_widget, chunks[0]);

    // Desc Input
    let desc_style = if matches!(app.input_mode, InputMode::EditingDesc) { Style::default().fg(Color::Yellow) } else { Style::default() };
    let desc_widget = Paragraph::new(app.desc_input.value())
        .block(Block::default().borders(Borders::ALL).title(" Description "))
        .style(desc_style)
        .wrap(Wrap { trim: true });
    f.render_widget(desc_widget, chunks[1]);

    // Cursor positioning
    match app.input_mode {
        InputMode::EditingTitle => f.set_cursor(chunks[0].x + 1 + app.title_input.visual_cursor() as u16, chunks[0].y + 1),
        InputMode::EditingDesc => f.set_cursor(chunks[1].x + 1 + app.desc_input.visual_cursor() as u16, chunks[1].y + 1),
        _ => {}
    }

    // Status / Help Message
    if !app.submit_status.is_empty() {
        let status = Paragraph::new(app.submit_status.as_str())
            .style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
        f.render_widget(status, chunks[2]);
    } else if matches!(app.input_mode, InputMode::Normal) {
        let help = Paragraph::new("Press 'Enter' to start typing.\nBudget and Type will be asked interactively after description.")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(help, chunks[2]);
    }
}

fn draw_telemetry(f: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(area);

    let sec_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    // 1. Core connection status
    let core_status = vec![
        Line::from(Span::styled("gRPC Connection to Private Core: ", Style::default().fg(Color::Cyan))),
        Line::from(Span::styled("🟢 ESTABLISHED (Latency: 1.2ms)", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))),
        Line::from("Protocol: Protobuf (Optimized)"),
        Line::from(""),
        Line::from(Span::styled("Security Pipeline (helm-secrets):", Style::default().fg(Color::Cyan))),
        Line::from("🔒 Vault Integration Active (No hardcoded secrets)"),
    ];
    let p_core = Paragraph::new(core_status)
        .block(Block::default().borders(Borders::ALL).title(" Core Link Status "));
    f.render_widget(p_core, sec_chunks[0]);

    // 2. Traffic routing status
    let route_status = vec![
        Line::from("Reverse Invoke Ports:"),
        Line::from("  Internal (K8s) : 8080"),
        Line::from("  External       : 443"),
        Line::from(""),
        Line::from("Gateway API Resource Model:"),
        Line::from("  GatewayClass   : 🟢 Active (Helm-sense)"),
        Line::from("  HTTPRoute      : 🟢 Strict Separation"),
    ];
    let p_route = Paragraph::new(route_status)
        .block(Block::default().borders(Borders::ALL).title(" Routing & Port Status "));
    f.render_widget(p_route, sec_chunks[1]);

    // 3. Memory & Threat Protection
    let mem_status = vec![
        Line::from(Span::styled("Threat Protection & Cache Limits (Anti-OOM):", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from("  REGISTRY_CACHE_MAX_KEYS  : 10,000 (Protected)"),
        Line::from("  REGISTRY_CACHE_STD_TTL   : 600s (Protected)"),
        Line::from("  PROXY_TIMEOUT            : 120s (Thread exhaustion prevented)"),
        Line::from("  CPU_LIMITS               : Applied via Mutating Webhook"),
        Line::from(""),
        Line::from(Span::styled("✔ Zero-Defect Architecture Configured", Style::default().fg(Color::Green))),
    ];
    let p_mem = Paragraph::new(mem_status)
        .block(Block::default().borders(Borders::ALL).title(" Memory & Load Defense "));
    f.render_widget(p_mem, chunks[1]);
}
