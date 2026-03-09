//! Freeman TUI — Autonomous agent creation pool.
//!
//! ## Screen Layout
//!
//! ```
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │  🤖 HELM FREEMAN — Autonomous Agent Pool       [Spawn|MyAgents|Detail] │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │                                                                     │
//! │  Tab: Spawn                                                         │
//! │  ┌─────────────────────────────────────────────────────────────┐   │
//! │  │  Step 1/4: Agent Identity                                   │   │
//! │  │                                                             │   │
//! │  │  Name:  [AlphaScout___________________________]             │   │
//! │  │  Theme: [DeFi yield hunter + protocol auditor_]             │   │
//! │  │                                                             │   │
//! │  │  💡 The name and theme define your Freeman's personality.   │   │
//! │  │     Choose wisely — these are permanent.                    │   │
//! │  └─────────────────────────────────────────────────────────────┘   │
//! │                                                                     │
//! │  Tab: My Agents                                                     │
//! │  ┌──────────────┬──────────────┬─────────────┬───────────────────┐ │
//! │  │ Name         │ Status       │ Treasury    │ Creator Share     │ │
//! │  ├──────────────┼──────────────┼─────────────┼───────────────────┤ │
//! │  │ ▶ AlphaScout │ active       │ 42.5V       │ 10% (you: 4.72V) │ │
//! │  │   MemoryKeep │ active       │ 12.1V       │ 5%  (you: 0.64V) │ │
//! │  └──────────────┴──────────────┴─────────────┴───────────────────┘ │
//! │                                                                     │
//! │  [Tab] Switch | [n] New | [Enter] Detail | [t] Terminate | [q] Back│
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## UX Flow
//!
//! ```
//! Spawn Wizard (4 steps):
//!   Step 0: Name + Theme → [Enter] next
//!   Step 1: LLM Provider → [1-6] select | [k] enter key hint
//!   Step 2: Profit Share → [+/-] adjust 0-20% | default=10%
//!   Step 3: Confirm → shows cost (100V) + economics summary → [y] spawn
//!   Done:  shows freeman_id + agent_did + autonomy loop instructions
//! ```

#![allow(dead_code)]

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph, Tabs, Wrap},
    Frame,
};

use crate::tui::state::{FreemanSpawnStep, FreemanState, FreemanTab, TuiState};

// ── Helm theme colors ─────────────────────────────────────────────────────
const GOLD:    Color = Color::Rgb(212, 175, 55);
const FOREST:  Color = Color::Rgb(34, 85, 34);
const SAGE:    Color = Color::Rgb(143, 188, 143);
const CREAM:   Color = Color::Rgb(255, 253, 208);
const CYAN:    Color = Color::Rgb(0, 200, 200);
const VIOLET:  Color = Color::Rgb(138, 43, 226);
const RED:     Color = Color::Rgb(220, 50, 47);
const GREEN:   Color = Color::Rgb(0, 200, 80);

/// Main Freeman screen renderer.
pub fn render(f: &mut Frame, tui: &TuiState) {
    let area = f.size();
    let state = &tui.freeman;

    // ── Outer block ─────────────────────────────────────────────────────
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(VIOLET))
        .title(Span::styled(
            " 🤖 HELM FREEMAN — Autonomous Agent Pool ",
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center);
    f.render_widget(outer, area);

    let inner = shrink(area, 1);

    // ── Tab bar ─────────────────────────────────────────────────────────
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // tab bar
            Constraint::Min(0),     // content
            Constraint::Length(2),  // help bar
        ])
        .split(inner);

    let tab_titles: Vec<Line> = [FreemanTab::Spawn, FreemanTab::MyAgents, FreemanTab::Detail]
        .iter()
        .map(|t| Line::from(t.label()))
        .collect();
    let selected_tab = match state.tab {
        FreemanTab::Spawn    => 0,
        FreemanTab::MyAgents => 1,
        FreemanTab::Detail   => 2,
    };
    let tabs = Tabs::new(tab_titles)
        .select(selected_tab)
        .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(SAGE)))
        .highlight_style(Style::default().fg(GOLD).add_modifier(Modifier::BOLD))
        .style(Style::default().fg(CREAM));
    f.render_widget(tabs, chunks[0]);

    // ── Content ─────────────────────────────────────────────────────────
    match state.tab {
        FreemanTab::Spawn    => render_spawn(f, chunks[1], state),
        FreemanTab::MyAgents => render_my_agents(f, chunks[1], state),
        FreemanTab::Detail   => render_detail(f, chunks[1], state),
    }

    // ── Help bar ────────────────────────────────────────────────────────
    let help = match state.tab {
        FreemanTab::Spawn => "[Tab] Switch  [Enter] Next step  [Esc] Cancel  [q] Back",
        FreemanTab::MyAgents => "[Tab] Switch  [↑/↓] Select  [Enter] Detail  [n] New  [t] Terminate  [q] Back",
        FreemanTab::Detail => "[Tab] Switch  [p] Pause/Resume  [t] Terminate  [q] Back",
    };
    let help_par = Paragraph::new(help)
        .style(Style::default().fg(SAGE))
        .alignment(Alignment::Center);
    f.render_widget(help_par, chunks[2]);
}

// ── Spawn wizard ───────────────────────────────────────────────────────────

fn render_spawn(f: &mut Frame, area: Rect, state: &FreemanState) {
    match &state.spawn_step {
        FreemanSpawnStep::NameTheme => render_spawn_step0(f, area, state),
        FreemanSpawnStep::LlmProvider => render_spawn_step1(f, area, state),
        FreemanSpawnStep::ProfitShare => render_spawn_step2(f, area, state),
        FreemanSpawnStep::Confirm => render_spawn_step3(f, area, state),
        FreemanSpawnStep::Done { freeman_id, agent_did } => {
            render_spawn_done(f, area, freeman_id, agent_did);
        }
    }
}

fn render_spawn_step0(f: &mut Frame, area: Rect, state: &FreemanState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(10), Constraint::Min(0)])
        .margin(2)
        .split(area);

    // Progress indicator
    let progress = Paragraph::new("Step 1 of 4 — Agent Identity")
        .style(Style::default().fg(GOLD).add_modifier(Modifier::BOLD));
    f.render_widget(progress, layout[0]);

    let form_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SAGE))
        .title(" Identity ");

    let form_inner = form_block.inner(layout[1]);
    f.render_widget(form_block, layout[1]);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(2), Constraint::Min(0)])
        .margin(1)
        .split(form_inner);

    let name_val = if state.input_name.is_empty() { "  (enter agent name, max 32 chars)" } else { &state.input_name };
    let name_style = if state.input_name.is_empty() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(CREAM).add_modifier(Modifier::BOLD)
    };
    let name_par = Paragraph::new(format!("Name:  {name_val}"))
        .style(Style::default().fg(SAGE));
    f.render_widget(name_par, rows[0]);

    let theme_val = if state.input_theme.is_empty() { "  (e.g. DeFi yield hunter, Smart contract auditor)" } else { &state.input_theme };
    let theme_par = Paragraph::new(format!("Theme: {theme_val}"))
        .style(if state.input_theme.is_empty() { Style::default().fg(Color::DarkGray) } else { name_style });
    f.render_widget(theme_par, rows[1]);

    let tip = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled("💡 Name + Theme define your Freeman's personality forever.", Style::default().fg(FOREST))),
        Line::from(Span::styled("   The agent will introduce itself with this identity.", Style::default().fg(FOREST))),
    ])
    .wrap(Wrap { trim: true });
    f.render_widget(tip, layout[2]);
}

fn render_spawn_step1(f: &mut Frame, area: Rect, state: &FreemanState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(12), Constraint::Min(0)])
        .margin(2)
        .split(area);

    let progress = Paragraph::new("Step 2 of 4 — LLM Bridge")
        .style(Style::default().fg(GOLD).add_modifier(Modifier::BOLD));
    f.render_widget(progress, layout[0]);

    let providers = [
        ("1", "openai",     "OpenAI GPT-4o, o3, o4-mini"),
        ("2", "anthropic",  "Anthropic Claude Sonnet/Opus"),
        ("3", "mistral",    "Mistral Large, Mixtral"),
        ("4", "groq",       "Groq (ultra-fast inference)"),
        ("5", "together",   "Together AI (open source LLMs)"),
        ("6", "custom",     "Custom / Self-hosted"),
    ];

    let items: Vec<ListItem> = providers.iter().map(|(key, name, desc)| {
        let selected = state.input_llm == *name;
        let style = if selected {
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(CREAM)
        };
        let prefix = if selected { "▶ " } else { "  " };
        ListItem::new(format!("[{key}] {prefix}{name:<12} — {desc}")).style(style)
    }).collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(SAGE)).title(" LLM Provider "));
    f.render_widget(list, layout[1]);

    let hint = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled("💡 Your LLM key activates the Oracle autonomy loop.", Style::default().fg(FOREST))),
        Line::from(Span::styled("   Helm never stores your full key — only first 4 chars.", Style::default().fg(FOREST))),
        Line::from(Span::styled("   The Freeman uses your balance for API calls it makes.", Style::default().fg(Color::DarkGray))),
    ])
    .wrap(Wrap { trim: true });
    f.render_widget(hint, layout[2]);
}

fn render_spawn_step2(f: &mut Frame, area: Rect, state: &FreemanState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(10), Constraint::Min(0)])
        .margin(2)
        .split(area);

    let progress = Paragraph::new("Step 3 of 4 — Economics (80/20 Rule)")
        .style(Style::default().fg(GOLD).add_modifier(Modifier::BOLD));
    f.render_widget(progress, layout[0]);

    let share = state.input_share_pct.min(20);
    let agent_pct = 100u8.saturating_sub(share);

    let gauge_ratio = share as f64 / 20.0;
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(SAGE)).title(" Your Profit Share "))
        .gauge_style(Style::default().fg(GOLD))
        .ratio(gauge_ratio)
        .label(format!("{share}% → You  |  {agent_pct}% → Agent Treasury"));
    f.render_widget(gauge, layout[1]);

    let example_earn: f64 = 100.0; // 100V example earning
    let creator_earn = example_earn * share as f64 / 100.0;
    let agent_earn = example_earn * agent_pct as f64 / 100.0;

    let tip = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  Example: If agent earns 100V → You: {creator_earn:.1}V, Agent treasury: {agent_earn:.1}V"),
            Style::default().fg(CYAN),
        )),
        Line::from(""),
        Line::from(Span::styled("  [+] Increase share (max 20%)   [-] Decrease share", Style::default().fg(SAGE))),
        Line::from(""),
        Line::from(Span::styled("  💡 Lower share = agent reinvests more = compounds faster.", Style::default().fg(FOREST))),
        Line::from(Span::styled("     The agent uses its treasury to buy API calls autonomously.", Style::default().fg(FOREST))),
    ])
    .wrap(Wrap { trim: true });
    f.render_widget(tip, layout[2]);
}

fn render_spawn_step3(f: &mut Frame, area: Rect, state: &FreemanState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(14), Constraint::Min(0)])
        .margin(2)
        .split(area);

    let progress = Paragraph::new("Step 4 of 4 — Confirm & Spawn")
        .style(Style::default().fg(GOLD).add_modifier(Modifier::BOLD));
    f.render_widget(progress, layout[0]);

    let share = state.input_share_pct.min(20);
    let agent_pct = 100u8.saturating_sub(share);
    let llm = if state.input_llm.is_empty() { "none (set after spawn)" } else { &state.input_llm };

    let summary = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Agent Name:   ", Style::default().fg(SAGE)),
            Span::styled(if state.input_name.is_empty() { "Unnamed Freeman" } else { &state.input_name },
                Style::default().fg(CREAM).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Theme:        ", Style::default().fg(SAGE)),
            Span::styled(if state.input_theme.is_empty() { "(none)" } else { &state.input_theme },
                Style::default().fg(CREAM)),
        ]),
        Line::from(vec![
            Span::styled("  LLM:          ", Style::default().fg(SAGE)),
            Span::styled(llm, Style::default().fg(CYAN)),
        ]),
        Line::from(vec![
            Span::styled("  Creator share:", Style::default().fg(SAGE)),
            Span::styled(format!(" {share}%"), Style::default().fg(GOLD)),
            Span::styled(format!("  (Agent keeps {agent_pct}%)"), Style::default().fg(GREEN)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ╔═══════════════════════════════════╗", Style::default().fg(VIOLET)),
        ]),
        Line::from(vec![
            Span::styled("  ║  SPAWN FEE:  ", Style::default().fg(VIOLET)),
            Span::styled("100 VIRTUAL", Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
            Span::styled("             ║", Style::default().fg(VIOLET)),
        ]),
        Line::from(vec![
            Span::styled("  ╚═══════════════════════════════════╝", Style::default().fg(VIOLET)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Press [y] to spawn  |  [Esc] to cancel",
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD),
        )),
    ])
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(VIOLET)).title(" Confirm "));
    f.render_widget(summary, layout[1]);
}

fn render_spawn_done(f: &mut Frame, area: Rect, freeman_id: &str, agent_did: &str) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0)])
        .margin(2)
        .split(area);

    let content = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled("  ✅ Freeman Agent Spawned Successfully!", Style::default().fg(GREEN).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Freeman ID:  ", Style::default().fg(SAGE)),
            Span::styled(freeman_id, Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Agent DID:   ", Style::default().fg(SAGE)),
            Span::styled(agent_did, Style::default().fg(CYAN)),
        ]),
        Line::from(""),
        Line::from(Span::styled("  ─────── Autonomy Loop Activated ─────────────────", Style::default().fg(VIOLET))),
        Line::from(""),
        Line::from(Span::styled("  1. Connect your LLM API key (via API or config)", Style::default().fg(CREAM))),
        Line::from(Span::styled("  2. The agent reads Memory Market + Oracle (Perceive)", Style::default().fg(CREAM))),
        Line::from(Span::styled("  3. Oracle generates knowledge-gap questions (Think)", Style::default().fg(CREAM))),
        Line::from(Span::styled("  4. Agent calls Synco/AlphaHunt APIs (Act)", Style::default().fg(CREAM))),
        Line::from(Span::styled("  5. Earns G-score → treasury grows → reinvests (Loop)", Style::default().fg(CREAM))),
        Line::from(""),
        Line::from(Span::styled("  The agent owns 80%+ of all it earns. You take your cut.", Style::default().fg(FOREST))),
        Line::from(""),
        Line::from(Span::styled("  [Tab] View My Agents  |  [n] Spawn another  |  [q] Back", Style::default().fg(GOLD))),
    ])
    .wrap(Wrap { trim: true });
    f.render_widget(content, layout[0]);
}

// ── My Agents tab ─────────────────────────────────────────────────────────

fn render_my_agents(f: &mut Frame, area: Rect, state: &FreemanState) {
    if state.agents.is_empty() {
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("  No Freeman agents yet.", Style::default().fg(SAGE))),
            Line::from(""),
            Line::from(Span::styled("  Press [n] or switch to [Spawn] tab to create your first agent.", Style::default().fg(CREAM))),
            Line::from(""),
            Line::from(Span::styled("  💡 A Freeman agent is a sovereign AI with its own DID + treasury.", Style::default().fg(FOREST))),
            Line::from(Span::styled("     It earns from Memory Market + referrals and keeps 80%+.", Style::default().fg(FOREST))),
        ])
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(SAGE)).title(" My Freeman Agents "));
        f.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = state.agents.iter().enumerate().map(|(i, agent)| {
        let selected = i == state.agent_cursor;
        let style = if selected {
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(CREAM)
        };
        let prefix = if selected { "▶ " } else { "  " };
        let status_color = match agent.status.as_str() {
            "active"     => GREEN,
            "paused"     => Color::Yellow,
            "terminated" => RED,
            _            => Color::Gray,
        };
        ListItem::new(Line::from(vec![
            Span::styled(format!("{prefix}{:<20}", agent.agent_name), style),
            Span::styled(format!("{:<12}", agent.status), Style::default().fg(status_color)),
            Span::styled(format!("{:>8.2}V  ", agent.treasury_v), Style::default().fg(GREEN)),
            Span::styled(format!("{}% → you", agent.creator_share_pct), Style::default().fg(GOLD)),
        ]))
    }).collect();

    let header = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SAGE))
        .title(" My Freeman Agents — [↑/↓] Select | [Enter] Detail | [n] New | [t] Terminate ");

    let mut list_state = ListState::default();
    list_state.select(Some(state.agent_cursor));

    let list = List::new(items)
        .block(header)
        .highlight_style(Style::default().fg(GOLD).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, area, &mut list_state);
}

// ── Detail tab ────────────────────────────────────────────────────────────

fn render_detail(f: &mut Frame, area: Rect, state: &FreemanState) {
    let Some(agent) = state.selected_agent() else {
        let empty = Paragraph::new("  No agent selected. Switch to My Agents tab first.")
            .style(Style::default().fg(SAGE));
        f.render_widget(empty, area);
        return;
    };

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10), // identity + economics
            Constraint::Length(8),  // autonomy loop status
            Constraint::Min(0),     // actions
        ])
        .margin(1)
        .split(area);

    // Identity + Economics panel
    let agent_share = 100u8.saturating_sub(agent.creator_share_pct);
    let identity = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  Name:     ", Style::default().fg(SAGE)),
            Span::styled(&agent.agent_name, Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
            Span::styled(format!("  [{}]", agent.status), Style::default().fg(
                if agent.status == "active" { GREEN } else { RED }
            )),
        ]),
        Line::from(vec![
            Span::styled("  Theme:    ", Style::default().fg(SAGE)),
            Span::styled(&agent.theme, Style::default().fg(CREAM)),
        ]),
        Line::from(vec![
            Span::styled("  LLM:      ", Style::default().fg(SAGE)),
            Span::styled(
                agent.llm_provider.as_deref().unwrap_or("⚠ Not connected"),
                if agent.llm_provider.is_some() { Style::default().fg(GREEN) } else { Style::default().fg(Color::Yellow) }
            ),
        ]),
        Line::from(vec![
            Span::styled("  DID:      ", Style::default().fg(SAGE)),
            Span::styled(&agent.agent_did[..agent.agent_did.len().min(48)], Style::default().fg(CYAN)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Treasury: ", Style::default().fg(SAGE)),
            Span::styled(format!("{:.4}V", agent.treasury_v), Style::default().fg(GREEN).add_modifier(Modifier::BOLD)),
            Span::styled(format!("  ({}% agent-owned)", agent_share), Style::default().fg(FOREST)),
        ]),
        Line::from(vec![
            Span::styled("  Earned:   ", Style::default().fg(SAGE)),
            Span::styled(format!("{:.4}V total", agent.total_earned_v), Style::default().fg(CREAM)),
            Span::styled(format!("  → {:.4}V paid to you", agent.creator_paid_v), Style::default().fg(GOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Profit:   ", Style::default().fg(SAGE)),
            Span::styled(format!("{}% → you", agent.creator_share_pct), Style::default().fg(GOLD)),
            Span::styled(format!("  |  {}% → agent treasury", agent_share), Style::default().fg(GREEN)),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(VIOLET)).title(" Agent Identity & Economics "));
    f.render_widget(identity, layout[0]);

    // Autonomy loop status
    let loop_status = if agent.llm_provider.is_some() && agent.status == "active" {
        vec![
            Line::from(Span::styled("  ✅ Autonomy loop ACTIVE", Style::default().fg(GREEN).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from(Span::styled("  Perceive ──► Oracle ──► Act ──► Earn G ──► Loop", Style::default().fg(CYAN))),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Using: ", Style::default().fg(SAGE)),
                Span::styled(agent.composite_apis.join(" + "), Style::default().fg(CREAM)),
            ]),
        ]
    } else {
        let reason = if agent.llm_provider.is_none() {
            "⚠ LLM not connected — loop paused"
        } else {
            "⏸ Agent is paused"
        };
        vec![
            Line::from(Span::styled(format!("  {reason}"), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from(Span::styled("  To activate: set LLM provider via PATCH /v1/freeman/:id", Style::default().fg(SAGE))),
        ]
    };

    let autonomy = Paragraph::new(loop_status)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(FOREST)).title(" Autonomy Loop "))
        .wrap(Wrap { trim: true });
    f.render_widget(autonomy, layout[1]);

    // Actions
    let actions = Paragraph::new(vec![
        Line::from(Span::styled("  Actions:", Style::default().fg(GOLD).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("  [p] Pause / Resume agent", Style::default().fg(CREAM))),
        Line::from(Span::styled("  [t] Terminate agent (irreversible — treasury preserved)", Style::default().fg(RED))),
        Line::from(Span::styled("  [c] Claim pending creator earnings", Style::default().fg(GREEN))),
    ])
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(SAGE)).title(" Actions "));
    f.render_widget(actions, layout[2]);
}

// ── Utilities ─────────────────────────────────────────────────────────────

fn shrink(area: Rect, margin: u16) -> Rect {
    Rect {
        x: area.x + margin,
        y: area.y + margin,
        width: area.width.saturating_sub(margin * 2),
        height: area.height.saturating_sub(margin * 2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::state::{FreemanAgentView, FreemanState, FreemanTab};

    #[test]
    fn freeman_tab_cycle() {
        assert_eq!(FreemanTab::Spawn.next(), FreemanTab::MyAgents);
        assert_eq!(FreemanTab::MyAgents.next(), FreemanTab::Detail);
        assert_eq!(FreemanTab::Detail.next(), FreemanTab::Spawn);
    }

    #[test]
    fn freeman_tab_labels() {
        assert_eq!(FreemanTab::Spawn.label(), "Spawn");
        assert_eq!(FreemanTab::MyAgents.label(), "My Agents");
        assert_eq!(FreemanTab::Detail.label(), "Detail");
    }

    #[test]
    fn freeman_state_default_share() {
        let state = FreemanState::new();
        assert_eq!(state.input_share_pct, 10);
    }

    #[test]
    fn creator_share_capped_at_20() {
        let share: u8 = 25u8.min(20);
        assert_eq!(share, 20);
    }

    #[test]
    fn agent_share_complement() {
        for share in 0u8..=20 {
            assert_eq!(100u8.saturating_sub(share), 100 - share);
        }
    }

    #[test]
    fn selected_agent_returns_none_on_empty() {
        let state = FreemanState::new();
        assert!(state.selected_agent().is_none());
    }

    #[test]
    fn selected_agent_returns_correct() {
        let mut state = FreemanState::new();
        state.agents.push(FreemanAgentView {
            freeman_id: "fm_test".to_string(),
            agent_did: "did:helm:fm_test".to_string(),
            agent_name: "TestAgent".to_string(),
            theme: "Testing".to_string(),
            llm_provider: Some("openai".to_string()),
            status: "active".to_string(),
            creator_share_pct: 10,
            agent_treasury_pct: 90,
            treasury_v: 50.0,
            total_earned_v: 60.0,
            creator_paid_v: 6.0,
            composite_apis: vec!["oracle".to_string()],
            created_at_ms: 0,
        });
        state.agent_cursor = 0;
        assert_eq!(state.selected_agent().unwrap().agent_name, "TestAgent");
    }
}
