//! Helm TUI — ratatui-based terminal dashboard.
//!
//! Entry point: [`run_tui`] starts the event loop.
//!
//! Screen routing:
//!   TuiScreen::Dashboard   → dashboard::render
//!   TuiScreen::AppHub(_)   → app_hub::render
//!   TuiScreen::Earn        → earn::render
//!   (other screens)        → placeholder stub

#![allow(dead_code)]

pub mod api_client;
pub mod app_hub;
pub mod components;
pub mod compute;
pub mod dashboard;
pub mod earn;
pub mod freeman;
pub mod marketplace;
pub mod onboarding;
pub mod pools;
pub mod settings;
pub mod state;
pub mod synthesis_tui;
pub mod theme;
pub mod topup;

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::sync::mpsc;

use state::{TuiScreen, TuiState, BalanceSnapshot, AppHubTab, FreemanTab, FreemanSpawnStep, PoolsTab, TopUpStage, SynthesisTab, OracleTierSelect, MarketplaceTab};
use api_client::TuiApiClient;

/// Events that can trigger a UI state change
pub enum TuiEvent {
    Key(KeyCode),
    Tick,
    BalanceRefreshed(BalanceSnapshot),
    ApiError(String),
}

pub fn run_tui(mut state: TuiState) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // [P1-A] Initialize Real API Client
    let api_client = Arc::new(TuiApiClient::new(state.gateway_url.clone(), state.did.clone(), [0u8; 32]));

    // [P2-C] Setup async communication channels
    let (tx, mut rx) = mpsc::unbounded_channel::<TuiEvent>();
    
    // Background task for periodic refresh
    let tx_ref = tx.clone();
    let client_ref = api_client.clone();
    tokio::spawn(async move {
        let mut last_refresh = Instant::now();
        loop {
            if last_refresh.elapsed() > Duration::from_secs(10) {
                if let Ok(balance) = client_ref.fetch_balance().await {
                    let _ = tx_ref.send(TuiEvent::BalanceRefreshed(balance));
                }
                last_refresh = Instant::now();
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });

    // Input handling task
    let tx_input = tx.clone();
    tokio::spawn(async move {
        loop {
            if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                if let Event::Key(key) = event::read().unwrap() {
                    let _ = tx_input.send(TuiEvent::Key(key.code));
                }
            }
            let _ = tx_input.send(TuiEvent::Tick);
        }
    });

    // Main UI Loop (Reactive)
    let _last_tick = Instant::now();
    loop {
        terminal.draw(|f| render_screen(f, &state))?;

        if let Ok(event) = rx.try_recv() {
            match event {
                TuiEvent::Key(code) => {
                    if code == KeyCode::Char('q') { break; }
                    handle_key(&mut state, code);
                }
                TuiEvent::Tick => {
                    state.tick = state.tick.wrapping_add(1);
                }
                TuiEvent::BalanceRefreshed(b) => {
                    state.balance = b;
                }
                TuiEvent::ApiError(e) => {
                    state.set_status(format!("API ERROR: {}", e));
                }
            }
        }

        if state.should_quit { break; }
        std::thread::sleep(Duration::from_millis(10));
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    Ok(())
}

fn render_screen(f: &mut ratatui::Frame, state: &TuiState) {
    use ratatui::layout::{Layout, Direction, Constraint};
    
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top Status Bar
            Constraint::Min(0),    // Body
        ])
        .split(f.size());

    components::status_bar::render(f, chunks[0], state);

    match state.screen {
        TuiScreen::Onboarding     => onboarding::render(f, chunks[1]),
        TuiScreen::Dashboard      => dashboard::render(f, state, chunks[1]),
        TuiScreen::AppHub(_)      => app_hub::render(f, state),
        TuiScreen::Marketplace    => marketplace::render(f, state),
        TuiScreen::Earn           => earn::render(f, state),
        TuiScreen::Freeman(_)     => freeman::render(f, state),
        TuiScreen::ApiNet         => compute::render(f, state),
        TuiScreen::Pools          => pools::render(f, state),
        TuiScreen::Settings       => settings::render(f, state),
        TuiScreen::TopUp          => topup::render(f, state),
        TuiScreen::SynthesisNet   => synthesis_tui::render(f, &state.synthesis_net),
    }
}


// ── Key dispatch ─────────────────────────────────────────────────────────

fn handle_key(state: &mut TuiState, code: KeyCode) {
    match state.screen {
        TuiScreen::Onboarding     => handle_onboarding(state, code),
        TuiScreen::Dashboard      => handle_dashboard(state, code),
        TuiScreen::AppHub(_)      => handle_app_hub(state, code),
        TuiScreen::Marketplace    => handle_marketplace(state, code),
        TuiScreen::Earn           => handle_earn(state, code),
        TuiScreen::Freeman(_)     => handle_freeman(state, code),
        TuiScreen::ApiNet         => handle_compute(state, code),
        TuiScreen::Pools          => handle_pools(state, code),
        TuiScreen::Settings       => handle_settings(state, code),
        TuiScreen::TopUp          => handle_topup(state, code),
        TuiScreen::SynthesisNet   => handle_synthesis_net(state, code),
    }
}

fn handle_onboarding(state: &mut TuiState, code: KeyCode) {
    match code {
        KeyCode::Enter => {
            state.screen = TuiScreen::Dashboard;
            state.set_status("Charter accepted. Sovereign session initialized.");
        }
        KeyCode::Char('q') | KeyCode::Esc => state.should_quit = true,
        _ => {}
    }
}

fn handle_dashboard(state: &mut TuiState, code: KeyCode) {
    match code {
        KeyCode::Char(c) => state.handle_dashboard_key(c),
        _ => {}
    }
}

fn handle_app_hub(state: &mut TuiState, code: KeyCode) {
    match code {
        KeyCode::Esc => state.back_to_dashboard(),
        KeyCode::Tab | KeyCode::Right => state.cycle_app_hub_tab(true),
        KeyCode::BackTab | KeyCode::Left => state.cycle_app_hub_tab(false),
        KeyCode::Char('1') => state.goto(TuiScreen::AppHub(AppHubTab::Oracle)),
        KeyCode::Char('2') => state.goto(TuiScreen::AppHub(AppHubTab::Cortex)),
        KeyCode::Char('3') => state.goto(TuiScreen::AppHub(AppHubTab::Memory)),
        KeyCode::Char('n') => state.app_hub.oracle_tier = OracleTierSelect::Nano,
        KeyCode::Char('s') => state.app_hub.oracle_tier = OracleTierSelect::Standard,
        KeyCode::Char('p') => state.app_hub.oracle_tier = OracleTierSelect::Pro,
        KeyCode::Char(c) => {
            // text input for query / memory write boxes
            state.app_hub.input.push(c);
        }
        KeyCode::Backspace => { state.app_hub.input.pop(); }
        KeyCode::Enter => {
            // TODO: fire API call; for now just clear input
            state.app_hub.input.clear();
        }
        KeyCode::Up => {
            if state.app_hub.memory_cursor > 0 {
                state.app_hub.memory_cursor -= 1;
            }
        }
        KeyCode::Down => {
            let max = state.app_hub.memory_keys.len().saturating_sub(1);
            if state.app_hub.memory_cursor < max {
                state.app_hub.memory_cursor += 1;
            }
        }
        _ => {}
    }
}

fn handle_marketplace(state: &mut TuiState, code: KeyCode) {
    match code {
        KeyCode::Esc => state.back_to_dashboard(),
        KeyCode::Tab => {
            state.marketplace.tab = match state.marketplace.tab {
                MarketplaceTab::Jobs    => MarketplaceTab::Compute,
                MarketplaceTab::Compute => MarketplaceTab::Storage,
                MarketplaceTab::Storage => MarketplaceTab::Hiring,
                MarketplaceTab::Hiring  => MarketplaceTab::Jobs,
            };
        }
        KeyCode::Up => {
            match state.marketplace.tab {
                MarketplaceTab::Jobs => {
                    if state.marketplace.post_cursor > 0 {
                        state.marketplace.post_cursor -= 1;
                    }
                }
                MarketplaceTab::Compute => {
                    if state.marketplace.compute_cursor > 0 {
                        state.marketplace.compute_cursor -= 1;
                    }
                }
                MarketplaceTab::Storage => {
                    if state.marketplace.storage_cursor > 0 {
                        state.marketplace.storage_cursor -= 1;
                    }
                }
                MarketplaceTab::Hiring => {
                    if state.marketplace.post_cursor > 0 {
                        state.marketplace.post_cursor -= 1;
                    }
                }
            }
        }
        KeyCode::Down => {
            match state.marketplace.tab {
                MarketplaceTab::Jobs => {
                    let max = state.marketplace.posts.len().saturating_sub(1);
                    if state.marketplace.post_cursor < max {
                        state.marketplace.post_cursor += 1;
                    }
                }
                MarketplaceTab::Compute => {
                    let max = state.marketplace.compute_listings.len().saturating_sub(1);
                    if state.marketplace.compute_cursor < max {
                        state.marketplace.compute_cursor += 1;
                    }
                }
                MarketplaceTab::Storage => {
                    let max = state.marketplace.storage_listings.len().saturating_sub(1);
                    if state.marketplace.storage_cursor < max {
                        state.marketplace.storage_cursor += 1;
                    }
                }
                MarketplaceTab::Hiring => {
                    // 3 mock posts
                    if state.marketplace.post_cursor < 2 {
                        state.marketplace.post_cursor += 1;
                    }
                }
            }
        }
        KeyCode::Char('c') => {
            state.marketplace.tab = MarketplaceTab::Compute;
        }
        KeyCode::Char('j') => {
            state.marketplace.tab = MarketplaceTab::Jobs;
        }
        KeyCode::Char('t') => {
            state.marketplace.tab = MarketplaceTab::Storage;
        }
        KeyCode::Char('h') => {
            state.marketplace.tab = MarketplaceTab::Hiring;
        }
        KeyCode::Char('a') => {
            if state.marketplace.tab == MarketplaceTab::Hiring {
                state.set_status("Human Operator application submitted. IdentityBond check in progress.");
            }
        }
        KeyCode::Char('s') => {
            // Spawn agent stub (Compute tab only)
            if state.marketplace.tab == MarketplaceTab::Compute {
                state.set_status("Spawn queued (stub — POST /v1/compute/spawn-agent)");
            }
        }
        KeyCode::Char('o') => {
            // Order storage stub (Storage tab only)
            if state.marketplace.tab == MarketplaceTab::Storage {
                state.set_status("Storage order queued (stub — POST /v1/storage/order)");
            }
        }
        KeyCode::Char('h') => {
            if state.marketplace.tab == MarketplaceTab::Compute {
                // Cycle spawn hours: 1 → 4 → 8 → 24 → 1
                state.marketplace.spawn_hours = match state.marketplace.spawn_hours {
                    1  => 4,
                    4  => 8,
                    8  => 24,
                    _  => 1,
                };
                state.set_status(&format!("Spawn duration: {}hr", state.marketplace.spawn_hours));
            }
        }
        KeyCode::Char('g') => {
            if state.marketplace.tab == MarketplaceTab::Storage {
                // Cycle GB: 1 → 10 → 50 → 100 → 1
                state.marketplace.storage_gb = match state.marketplace.storage_gb {
                    1   => 10,
                    10  => 50,
                    50  => 100,
                    _   => 1,
                };
                state.set_status(&format!("Storage: {} GB", state.marketplace.storage_gb));
            }
        }
        KeyCode::Char('m') => {
            if state.marketplace.tab == MarketplaceTab::Storage {
                // Cycle months: 1 → 3 → 6 → 12 → 1
                state.marketplace.storage_months = match state.marketplace.storage_months {
                    1  => 3,
                    3  => 6,
                    6  => 12,
                    _  => 1,
                };
                state.set_status(&format!("Duration: {} months", state.marketplace.storage_months));
            }
        }
        KeyCode::Char('r') => {
            if state.marketplace.tab == MarketplaceTab::Jobs {
                state.set_status("Brokering request linked to your DID. 20% commission activated.");
            }
        }
        _ => {}
    }
}

fn handle_earn(state: &mut TuiState, code: KeyCode) {
    match code {
        KeyCode::Esc => state.back_to_dashboard(),
        KeyCode::Char('r') => state.set_status("Refreshed"),
        KeyCode::Char('c') => state.set_status("Claim sent (stub)"),
        _ => {}
    }
}

fn handle_freeman(state: &mut TuiState, code: KeyCode) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') => state.back_to_dashboard(),
        KeyCode::Tab => {
            // Advance Freeman tab
            let next = state.freeman.tab.next();
            state.screen = TuiScreen::Freeman(next);
            state.freeman.tab = next;
        }
        KeyCode::Up => {
            if state.freeman.tab == FreemanTab::MyAgents && state.freeman.agent_cursor > 0 {
                state.freeman.agent_cursor -= 1;
            }
        }
        KeyCode::Down => {
            if state.freeman.tab == FreemanTab::MyAgents {
                let max = state.freeman.agents.len().saturating_sub(1);
                if state.freeman.agent_cursor < max {
                    state.freeman.agent_cursor += 1;
                }
            }
        }
        KeyCode::Enter => {
            match (&state.freeman.tab, &state.freeman.spawn_step) {
                (FreemanTab::Spawn, FreemanSpawnStep::NameTheme) => {
                    state.freeman.spawn_step = FreemanSpawnStep::LlmProvider;
                    state.set_status("Step 2: Choose your LLM provider");
                }
                (FreemanTab::Spawn, FreemanSpawnStep::LlmProvider) => {
                    state.freeman.spawn_step = FreemanSpawnStep::ProfitShare;
                    state.set_status("Step 3: Set profit share (0–20%)");
                }
                (FreemanTab::Spawn, FreemanSpawnStep::ProfitShare) => {
                    state.freeman.spawn_step = FreemanSpawnStep::Confirm;
                    state.set_status("Step 4: Review and confirm");
                }
                (FreemanTab::MyAgents, _) => {
                    // Navigate to Detail tab for selected agent
                    let next = FreemanTab::Detail;
                    state.screen = TuiScreen::Freeman(next);
                    state.freeman.tab = next;
                }
                _ => {}
            }
        }
        KeyCode::Char('y') => {
            // Confirm spawn
            if matches!(state.freeman.spawn_step, FreemanSpawnStep::Confirm) {
                // Stub: simulate spawn success
                let fm_id = format!("fm_{:08x}", state.freeman.created_at_hint());
                let agent_did = format!("did:helm:fm{:016x}", state.freeman.created_at_hint());
                state.freeman.spawn_step = FreemanSpawnStep::Done {
                    freeman_id: fm_id,
                    agent_did,
                };
                state.set_status("Freeman agent spawned! (stub — calls POST /v1/freeman/spawn)");
            }
        }
        KeyCode::Char('n') => {
            // New spawn — reset wizard
            state.freeman.spawn_step = FreemanSpawnStep::NameTheme;
            state.freeman.input_name.clear();
            state.freeman.input_theme.clear();
            state.freeman.input_llm.clear();
            state.freeman.input_share_pct = 10;
            let spawn_tab = FreemanTab::Spawn;
            state.screen = TuiScreen::Freeman(spawn_tab);
            state.freeman.tab = spawn_tab;
            state.set_status("New Freeman spawn wizard started");
        }
        KeyCode::Char('+') | KeyCode::Char(']') => {
            if matches!(state.freeman.spawn_step, FreemanSpawnStep::ProfitShare) {
                if state.freeman.input_share_pct < 20 {
                    state.freeman.input_share_pct += 1;
                }
            }
        }
        KeyCode::Char('-') | KeyCode::Char('[') => {
            if matches!(state.freeman.spawn_step, FreemanSpawnStep::ProfitShare) {
                if state.freeman.input_share_pct > 0 {
                    state.freeman.input_share_pct -= 1;
                }
            }
        }
        KeyCode::Char(c @ '1'..='6') => {
            if matches!(state.freeman.spawn_step, FreemanSpawnStep::LlmProvider) {
                let providers = ["openai", "anthropic", "mistral", "groq", "together", "custom"];
                let idx = (c as u8 - b'1') as usize;
                if let Some(p) = providers.get(idx) {
                    state.freeman.input_llm = p.to_string();
                    state.set_status(&format!("LLM: {p} selected"));
                }
            }
        }
        KeyCode::Char(c) => {
            // Text input for name/theme
            match &state.freeman.spawn_step {
                FreemanSpawnStep::NameTheme => {
                    if state.freeman.input_name.len() < 32 {
                        state.freeman.input_name.push(c);
                    }
                }
                _ => {}
            }
        }
        KeyCode::Backspace => {
            match &state.freeman.spawn_step {
                FreemanSpawnStep::NameTheme => { state.freeman.input_name.pop(); }
                _ => {}
            }
        }
        _ => {}
    }
}

// ── Compute handler ───────────────────────────────────────────────────────

fn handle_compute(state: &mut TuiState, code: KeyCode) {
    let n = state.compute.providers.len();
    match code {
        KeyCode::Esc | KeyCode::Char('q') => state.back_to_dashboard(),
        KeyCode::Up => {
            if state.compute.cursor > 0 { state.compute.cursor -= 1; }
        }
        KeyCode::Down => {
            if state.compute.cursor + 1 < n { state.compute.cursor += 1; }
        }
        KeyCode::Enter => {
            let available = state.compute.providers.get(state.compute.cursor)
                .map(|p| p.available).unwrap_or(false);
            if available { state.compute.confirming = true; }
            else { state.set_status("Provider unavailable"); }
        }
        KeyCode::Char('y') if state.compute.confirming => {
            state.compute.confirming = false;
            state.set_status("Spawn request submitted (stub)");
        }
        KeyCode::Esc if state.compute.confirming => {
            state.compute.confirming = false;
        }
        KeyCode::Char('+') | KeyCode::Char(']') => {
            let h: f64 = state.compute.spawn_hours_input.parse().unwrap_or(1.0);
            state.compute.spawn_hours_input = format!("{}", (h + 1.0).min(720.0));
        }
        KeyCode::Char('-') | KeyCode::Char('[') => {
            let h: f64 = state.compute.spawn_hours_input.parse().unwrap_or(1.0);
            state.compute.spawn_hours_input = format!("{:.0}", (h - 1.0).max(1.0));
        }
        KeyCode::Char('s') => {
            // Sort by price ascending
            state.compute.providers.sort_by(|a, b| a.price_v_hr.partial_cmp(&b.price_v_hr).unwrap());
            state.compute.cursor = 0;
        }
        _ => {}
    }
}

// ── Pools handler ─────────────────────────────────────────────────────────

fn handle_pools(state: &mut TuiState, code: KeyCode) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') => state.back_to_dashboard(),
        KeyCode::Tab => {
            state.pools.tab = state.pools.tab.next();
        }
        KeyCode::Up => {
            match state.pools.tab {
                PoolsTab::Browse | PoolsTab::Contracts => {
                    if state.pools.cursor > 0 { state.pools.cursor -= 1; }
                }
                _ => {}
            }
        }
        KeyCode::Down => {
            match state.pools.tab {
                PoolsTab::Browse => {
                    let max = state.pools.pools.len().saturating_sub(1);
                    if state.pools.cursor < max { state.pools.cursor += 1; }
                }
                PoolsTab::Contracts => {
                    // 3 mock contracts
                    if state.pools.cursor < 2 { state.pools.cursor += 1; }
                }
                _ => {}
            }
        }
        KeyCode::Char('h') => {
            if state.pools.tab == PoolsTab::Contracts {
                state.set_status("Human Principal application submitted (IdentityBond Lvl 2+ required)");
            }
        }
        KeyCode::Char('j') => {
            if let Some(pool) = state.pools.selected() {
                if pool.status == "open" {
                    state.set_status(format!("Join request sent for {}", pool.name));
                } else {
                    state.set_status("Pool is closed");
                }
            }
        }
        KeyCode::Enter if matches!(state.pools.tab, PoolsTab::Create) => {
            if !state.pools.input_name.is_empty() {
                state.set_status(format!("Pool '{}' creation submitted", state.pools.input_name));
                state.pools.input_name.clear();
            } else {
                state.set_status("Enter a pool name first");
            }
        }
        KeyCode::Char(c) if matches!(state.pools.tab, PoolsTab::Create) => {
            state.pools.input_name.push(c);
        }
        KeyCode::Backspace if matches!(state.pools.tab, PoolsTab::Create) => {
            state.pools.input_name.pop();
        }
        _ => {}
    }
}

// ── Settings handler ──────────────────────────────────────────────────────

fn handle_settings(state: &mut TuiState, code: KeyCode) {
    if state.settings.editing_name {
        match code {
            KeyCode::Enter => {
                state.settings.display_name = state.settings.input_buffer.clone();
                state.settings.editing_name = false;
                state.settings.input_buffer.clear();
                state.set_status("Display name saved");
            }
            KeyCode::Esc => {
                state.settings.editing_name = false;
                state.settings.input_buffer.clear();
            }
            KeyCode::Char(c) => { state.settings.input_buffer.push(c); }
            KeyCode::Backspace => { state.settings.input_buffer.pop(); }
            _ => {}
        }
        return;
    }
    match code {
        KeyCode::Esc | KeyCode::Char('q') => state.back_to_dashboard(),
        KeyCode::Char('e') => {
            state.settings.editing_name = true;
            state.settings.input_buffer = state.settings.display_name.clone();
        }
        KeyCode::Char('c') => {
            state.set_status(format!("DID copied: {}", state.did_short));
        }
        KeyCode::Char('r') => {
            state.set_status("Referral link copied");
        }
        _ => {}
    }
}

// ── TopUp handler ─────────────────────────────────────────────────────────

fn handle_topup(state: &mut TuiState, code: KeyCode) {
    match state.topup.stage {
        TopUpStage::SelectMethod => match code {
            KeyCode::Esc | KeyCode::Char('q') => state.back_to_dashboard(),
            KeyCode::Char('1') => state.topup.method_idx = 0,
            KeyCode::Char('2') => state.topup.method_idx = 1,
            KeyCode::Char('3') => state.topup.method_idx = 2,
            KeyCode::Enter     => state.topup.stage = TopUpStage::EnterAmount,
            _ => {}
        },
        TopUpStage::EnterAmount => match code {
            KeyCode::Esc       => state.topup.stage = TopUpStage::SelectMethod,
            KeyCode::Char('q') => state.back_to_dashboard(),
            KeyCode::Enter     => {
                if state.topup.parsed_amount() > 0.0 {
                    state.topup.stage = TopUpStage::Confirm;
                } else {
                    state.set_status("Enter a valid amount > 0");
                }
            }
            KeyCode::Char(c @ '0'..='9') | KeyCode::Char(c @ '.') => {
                // Only allow one decimal point
                if c != '.' || !state.topup.amount_input.contains('.') {
                    state.topup.amount_input.push(c);
                }
            }
            KeyCode::Backspace => { state.topup.amount_input.pop(); }
            _ => {}
        },
        TopUpStage::Confirm => match code {
            KeyCode::Esc       => state.topup.stage = TopUpStage::EnterAmount,
            KeyCode::Char('q') => state.back_to_dashboard(),
            KeyCode::Char('y') => {
                state.topup.stage = TopUpStage::Submitted;
                state.topup.tx_status = Some("Submitted to gateway...".into());
            }
            _ => {}
        },
        TopUpStage::Submitted => match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                state.topup.stage = TopUpStage::SelectMethod;
                state.back_to_dashboard();
            }
            _ => {}
        },
    }
}

// ── SynthesisNet handler ──────────────────────────────────────────────────

fn handle_synthesis_net(state: &mut TuiState, code: KeyCode) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') => state.back_to_dashboard(),
        KeyCode::Tab => {
            let next = state.synthesis_net.tab.next();
            state.synthesis_net.tab = next;
        }
        KeyCode::Up => {
            match state.synthesis_net.tab {
                SynthesisTab::Browse => {
                    if state.synthesis_net.browse_cursor > 0 {
                        state.synthesis_net.browse_cursor -= 1;
                    }
                }
                SynthesisTab::Catalog => {
                    if state.synthesis_net.catalog_cursor > 0 {
                        state.synthesis_net.catalog_cursor -= 1;
                    }
                }
                SynthesisTab::Build => {
                    if state.synthesis_net.build.catalog_cursor > 0 {
                        state.synthesis_net.build.catalog_cursor -= 1;
                    }
                }
                _ => {}
            }
        }
        KeyCode::Down => {
            match state.synthesis_net.tab {
                SynthesisTab::Browse => {
                    let max = state.synthesis_net.listings.len().saturating_sub(1);
                    if state.synthesis_net.browse_cursor < max {
                        state.synthesis_net.browse_cursor += 1;
                    }
                }
                SynthesisTab::Catalog => {
                    // 15 catalog entries
                    if state.synthesis_net.catalog_cursor < 14 {
                        state.synthesis_net.catalog_cursor += 1;
                    }
                }
                SynthesisTab::Build => {
                    if state.synthesis_net.build.catalog_cursor < 14 {
                        state.synthesis_net.build.catalog_cursor += 1;
                    }
                }
                _ => {}
            }
        }
        KeyCode::Char('c') => {
            if state.synthesis_net.tab == SynthesisTab::Browse {
                state.synthesis_net.show_snippet = !state.synthesis_net.show_snippet;
            }
        }
        KeyCode::Char('r') => {
            if state.synthesis_net.tab == SynthesisTab::Browse {
                if let Some(listing) = state.synthesis_net.listings.get(state.synthesis_net.browse_cursor) {
                    state.set_status(&format!("Brokering link generated for '{}' (20% commission active)", listing.name));
                    // In a real implementation, this would fire the api_client.broker_synthesis call
                }
            }
        }
        KeyCode::Char(' ') => {
            // Toggle component in Build step 1
            if state.synthesis_net.tab == SynthesisTab::Build
                && state.synthesis_net.build.step == 1
            {
                // Catalog entry at cursor
                let catalog_ids = [
                    "helm/oracle", "helm/cortex", "helm/memory", "helm/synco",
                    "helm/alpha", "helm/grg", "helm/oracle",
                    "ext/coingecko", "ext/defillama", "ext/etherscan",
                    "ext/thegraph", "ext/newsapi", "ext/github",
                    "depin/akash", "depin/flux",
                ];
                if let Some(&id) = catalog_ids.get(state.synthesis_net.build.catalog_cursor) {
                    let id = id.to_string();
                    let comps = &mut state.synthesis_net.build.selected_comps;
                    if comps.contains(&id) {
                        comps.retain(|c| *c != id);
                    } else {
                        comps.push(id);
                    }
                }
            }
        }
        KeyCode::Enter => {
            match state.synthesis_net.tab {
                SynthesisTab::Browse => {
                    state.synthesis_net.show_snippet = true;
                }
                SynthesisTab::Build => {
                    let build = &mut state.synthesis_net.build;
                    match build.step {
                        0 => {
                            if !build.name_input.is_empty() {
                                build.step = 1;
                                state.set_status("Step 2: Select components [Space] to toggle");
                            } else {
                                state.set_status("Enter a name first");
                            }
                        }
                        1 => {
                            if !build.selected_comps.is_empty() {
                                build.step = 2;
                                build.price_input = "0.5".into();
                                state.set_status("Step 3: Set per-call price");
                            } else {
                                state.set_status("Select at least 1 component");
                            }
                        }
                        2 => {
                            if build.price_v() >= 0.1 {
                                build.step = 3;
                                state.set_status("Step 4: Confirm — press [y] to submit");
                            } else {
                                state.set_status("Minimum price is 0.1V");
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        KeyCode::Char('y') => {
            if state.synthesis_net.tab == SynthesisTab::Build
                && state.synthesis_net.build.step == 3
                && !state.synthesis_net.build.submitted
            {
                state.synthesis_net.build.submitted = true;
                state.synthesis_net.build.result_endpoint = Some(
                    format!("https://api.helm.io/v1/synth/did:helm:you/{}",
                        state.synthesis_net.build.name_input.to_lowercase().replace(' ', "_"))
                );
                state.set_status("Synthesis API submitted! (calls POST /v1/synthesis/create)");
            }
        }
        KeyCode::Char(c) => {
            // Name/description/price input in Build tab
            if state.synthesis_net.tab == SynthesisTab::Build {
                let build = &mut state.synthesis_net.build;
                match build.step {
                    0 => {
                        if build.name_input.len() < 48 { build.name_input.push(c); }
                    }
                    2 => {
                        // Price input
                        if c.is_ascii_digit() || (c == '.' && !build.price_input.contains('.')) {
                            build.price_input.push(c);
                        }
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Backspace => {
            if state.synthesis_net.tab == SynthesisTab::Build {
                let build = &mut state.synthesis_net.build;
                match build.step {
                    0 => { build.name_input.pop(); }
                    2 => { build.price_input.pop(); }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use state::TuiState;

    fn make_state() -> TuiState {
        TuiState::new("did:helm:testDID1234567890ABCD".into(), "http://localhost:8080", true)
    }

    #[test]
    fn dashboard_key_a_goes_to_app_hub() {
        let mut state = make_state();
        handle_dashboard(&mut state, KeyCode::Char('a'));
        assert!(matches!(state.screen, TuiScreen::AppHub(_)));
    }

    #[test]
    fn dashboard_key_7_goes_to_earn() {
        let mut state = make_state();
        handle_dashboard(&mut state, KeyCode::Char('7'));
        assert_eq!(state.screen, TuiScreen::Earn);
    }

    #[test]
    fn app_hub_esc_returns_to_dashboard() {
        let mut state = make_state();
        state.goto(TuiScreen::AppHub(AppHubTab::Oracle));
        handle_app_hub(&mut state, KeyCode::Esc);
        assert_eq!(state.screen, TuiScreen::Dashboard);
    }

    #[test]
    fn app_hub_tab_key_cycles() {
        let mut state = make_state();
        state.goto(TuiScreen::AppHub(AppHubTab::Oracle));
        handle_app_hub(&mut state, KeyCode::Tab);
        assert_eq!(state.screen, TuiScreen::AppHub(AppHubTab::Cortex));
    }

    #[test]
    fn app_hub_right_arrow_cycles() {
        let mut state = make_state();
        state.goto(TuiScreen::AppHub(AppHubTab::Cortex));
        handle_app_hub(&mut state, KeyCode::Right);
        assert_eq!(state.screen, TuiScreen::AppHub(AppHubTab::Memory));
    }

    #[test]
    fn app_hub_tier_keys() {
        let mut state = make_state();
        state.goto(TuiScreen::AppHub(AppHubTab::Oracle));
        handle_app_hub(&mut state, KeyCode::Char('n'));
        assert_eq!(state.app_hub.oracle_tier, OracleTierSelect::Nano);
        handle_app_hub(&mut state, KeyCode::Char('p'));
        assert_eq!(state.app_hub.oracle_tier, OracleTierSelect::Pro);
        handle_app_hub(&mut state, KeyCode::Char('s'));
        assert_eq!(state.app_hub.oracle_tier, OracleTierSelect::Standard);
    }

    #[test]
    fn app_hub_input_typing() {
        let mut state = make_state();
        state.goto(TuiScreen::AppHub(AppHubTab::Oracle));
        handle_app_hub(&mut state, KeyCode::Char('h'));
        handle_app_hub(&mut state, KeyCode::Char('i'));
        assert_eq!(state.app_hub.input, "hi");
        handle_app_hub(&mut state, KeyCode::Backspace);
        assert_eq!(state.app_hub.input, "h");
    }

    #[test]
    fn earn_esc_returns_dashboard() {
        let mut state = make_state();
        state.goto(TuiScreen::Earn);
        handle_earn(&mut state, KeyCode::Esc);
        assert_eq!(state.screen, TuiScreen::Dashboard);
    }

    #[test]
    fn stub_esc_returns_dashboard() {
        let mut state = make_state();
        state.goto(TuiScreen::Marketplace);
        handle_marketplace(&mut state, KeyCode::Esc);
        assert_eq!(state.screen, TuiScreen::Dashboard);
    }

    #[test]
    fn dashboard_q_sets_quit() {
        let mut state = make_state();
        handle_dashboard(&mut state, KeyCode::Char('q'));
        assert!(state.should_quit);
    }
}
