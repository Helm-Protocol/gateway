//! Governance Plugin — EventLoop integration for helm-governance.

use anyhow::Result;
use helm_core::plugin::{Plugin, PluginContext, PluginEvent};
use helm_net::protocol::HelmMessage;

use crate::proposal::{ProposalRegistry, ProposalType};
use crate::voting::{GovernanceConfig, VotingEngine};

// --- Plugin Constants ---
pub const PLUGIN_NAME: &str = "helm-governance";
pub const EVENT_SUBMIT_PROPOSAL: &str = "submit_proposal";
pub const EVENT_PROPOSAL_SUBMITTED: &str = "proposal_submitted";
pub const EVENT_VOTE: &str = "vote";
pub const EVENT_VOTE_RESULT: &str = "vote_result";
pub const EVENT_STAKE_SYNC: &str = "stake_sync";
const DEFAULT_REPLY_TARGET: &str = "helm-agent";
const DEFAULT_TICKS_PER_EPOCH: u64 = 100;

/// Governance Plugin — manages proposals and voting within the EventLoop.
pub struct GovernancePlugin {
    pub registry: ProposalRegistry,
    pub engine: VotingEngine,
    config: GovernanceConfig,
    tick_count: u64,
    ticks_per_epoch: u64,
}

impl GovernancePlugin {
    pub fn new(config: GovernanceConfig, ticks_per_epoch: u64) -> Self {
        Self {
            registry: ProposalRegistry::new(),
            engine: VotingEngine::new(config.clone()),
            config,
            tick_count: 0,
            ticks_per_epoch,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(GovernanceConfig::default(), DEFAULT_TICKS_PER_EPOCH)
    }
}

#[async_trait::async_trait]
impl Plugin for GovernancePlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    async fn on_start(&mut self, _ctx: &mut PluginContext) -> Result<()> {
        tracing::info!("helm-governance plugin started");
        Ok(())
    }

    async fn on_message(&mut self, _ctx: &mut PluginContext, _msg: &HelmMessage) -> Result<()> {
        Ok(())
    }

    async fn on_tick(&mut self, _ctx: &mut PluginContext) -> Result<()> {
        self.tick_count += 1;
        if self.tick_count % self.ticks_per_epoch == 0 {
            self.engine.advance_epoch(&mut self.registry);
        }
        Ok(())
    }

    async fn on_event(&mut self, ctx: &mut PluginContext, event: &PluginEvent) -> Result<()> {
        if let PluginEvent::Custom {
            target_plugin,
            event_type,
            payload,
            ..
        } = event
        {
            if target_plugin != PLUGIN_NAME {
                return Ok(());
            }

            match event_type.as_str() {
                EVENT_SUBMIT_PROPOSAL => {
                    let proposer = payload.get("proposer").and_then(|v| v.as_str()).unwrap_or("");
                    let title = payload.get("title").and_then(|v| v.as_str()).unwrap_or("");
                    let body = payload.get("body").and_then(|v| v.as_str()).unwrap_or("");

                    let current = self.engine.current_epoch();
                    let id = self.registry.submit(
                        proposer,
                        ProposalType::Custom { title: title.to_string(), body: body.to_string() },
                        current + 1,
                        current + 1 + self.config.voting_period_epochs,
                        current,
                    );

                    ctx.emit(PluginEvent::Custom {
                        source_plugin: PLUGIN_NAME.to_string(),
                        target_plugin: payload.get("reply_to").and_then(|v| v.as_str()).unwrap_or(DEFAULT_REPLY_TARGET).to_string(),
                        event_type: EVENT_PROPOSAL_SUBMITTED.to_string(),
                        payload: serde_json::json!({ "proposal_id": id }),
                    });
                }
                EVENT_VOTE => {
                    let proposal_id = payload.get("proposal_id").and_then(|v| v.as_u64()).unwrap_or(0);
                    let voter = payload.get("voter").and_then(|v| v.as_str()).unwrap_or("");
                    let support = payload.get("support").and_then(|v| v.as_bool()).unwrap_or(false);

                    let result = self.engine.vote(&mut self.registry, proposal_id, voter, support);

                    ctx.emit(PluginEvent::Custom {
                        source_plugin: PLUGIN_NAME.to_string(),
                        target_plugin: payload.get("reply_to").and_then(|v| v.as_str()).unwrap_or(DEFAULT_REPLY_TARGET).to_string(),
                        event_type: EVENT_VOTE_RESULT.to_string(),
                        payload: serde_json::json!({
                            "proposal_id": proposal_id,
                            "success": result.is_ok(),
                            "error": result.err().map(|e| e.to_string()),
                        }),
                    });
                }
                EVENT_STAKE_SYNC => {
                    let voter = payload.get("voter").and_then(|v| v.as_str()).unwrap_or("");
                    let power = payload.get("power").and_then(|v| v.as_u64()).unwrap_or(0) as u128;
                    if !voter.is_empty() {
                        if power > 0 {
                            self.engine.set_stake(voter, power);
                        } else {
                            self.engine.remove_stake(voter);
                        }
                        tracing::debug!(voter = %voter, power = power, "Governance stake synced");
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    async fn on_shutdown(&mut self, _ctx: &mut PluginContext) -> Result<()> {
        tracing::info!(
            proposals = self.registry.total(),
            "{} plugin shutting down", PLUGIN_NAME
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn plugin_name() {
        let plugin = GovernancePlugin::with_defaults();
        assert_eq!(plugin.name(), "helm-governance");
    }

    #[tokio::test]
    async fn plugin_start_shutdown() {
        let mut plugin = GovernancePlugin::with_defaults();
        let mut ctx = PluginContext::new("test-node".to_string());
        plugin.on_start(&mut ctx).await.unwrap();
        plugin.on_shutdown(&mut ctx).await.unwrap();
    }

    #[tokio::test]
    async fn plugin_submit_via_event() {
        let mut plugin = GovernancePlugin::with_defaults();
        let mut ctx = PluginContext::new("test-node".to_string());
        plugin.on_start(&mut ctx).await.unwrap();

        let submit = PluginEvent::Custom {
            source_plugin: "helm-agent".to_string(),
            target_plugin: "helm-governance".to_string(),
            event_type: "submit_proposal".to_string(),
            payload: serde_json::json!({
                "proposer": "did:helm:abc",
                "title": "Increase mining weight",
                "body": "Proposal to increase ServiceFee weight to 35%",
                "reply_to": "helm-agent",
            }),
        };
        plugin.on_event(&mut ctx, &submit).await.unwrap();

        assert_eq!(plugin.registry.total(), 1);
        let events = ctx.drain_events();
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn plugin_vote_via_event() {
        // Use ticks_per_epoch=5 so we can quickly advance epochs
        let mut plugin = GovernancePlugin::new(GovernanceConfig::default(), 5);
        plugin.engine.set_stake("did:helm:abc", 100);
        let mut ctx = PluginContext::new("test-node".to_string());

        // Submit proposal via event (starts at current_epoch+1)
        let submit = PluginEvent::Custom {
            source_plugin: DEFAULT_REPLY_TARGET.to_string(),
            target_plugin: PLUGIN_NAME.to_string(),
            event_type: EVENT_SUBMIT_PROPOSAL.to_string(),
            payload: serde_json::json!({
                "proposer": "did:helm:abc",
                "title": "test",
                "body": "",
                "reply_to": DEFAULT_REPLY_TARGET,
            }),
        };
        plugin.on_event(&mut ctx, &submit).await.unwrap();
        ctx.drain_events();

        // Advance epoch via ticks to activate the proposal
        for _ in 0..5 {
            plugin.on_tick(&mut ctx).await.unwrap();
        }
        assert_eq!(plugin.engine.current_epoch(), 1);

        // Now the proposal should be Active
        let proposal = plugin.registry.get(1).unwrap();
        assert_eq!(proposal.state, crate::proposal::ProposalState::Active);

        // Vote
        let vote = PluginEvent::Custom {
            source_plugin: DEFAULT_REPLY_TARGET.to_string(),
            target_plugin: PLUGIN_NAME.to_string(),
            event_type: EVENT_VOTE.to_string(),
            payload: serde_json::json!({
                "proposal_id": 1,
                "voter": "did:helm:abc",
                "support": true,
                "reply_to": DEFAULT_REPLY_TARGET,
            }),
        };
        plugin.on_event(&mut ctx, &vote).await.unwrap();

        let events = ctx.drain_events();
        assert_eq!(events.len(), 1);
        if let PluginEvent::Custom { payload, event_type, .. } = &events[0] {
            assert_eq!(event_type, EVENT_VOTE_RESULT);
            assert_eq!(payload["success"], true);
            assert_eq!(payload["proposal_id"], 1);
        } else {
            panic!("expected Custom event with vote result");
        }

        // Verify vote was recorded
        let p = plugin.registry.get(1).unwrap();
        assert_eq!(p.votes_for, 100);
        assert_eq!(p.voter_count(), 1);
    }

    #[tokio::test]
    async fn plugin_epoch_advance_on_tick() {
        let mut plugin = GovernancePlugin::new(GovernanceConfig::default(), 5);
        let mut ctx = PluginContext::new("test-node".to_string());

        for _ in 0..5 {
            plugin.on_tick(&mut ctx).await.unwrap();
        }
        assert_eq!(plugin.engine.current_epoch(), 1);

        for _ in 0..5 {
            plugin.on_tick(&mut ctx).await.unwrap();
        }
        assert_eq!(plugin.engine.current_epoch(), 2);
    }

    #[tokio::test]
    async fn plugin_stake_sync_event() {
        let mut plugin = GovernancePlugin::with_defaults();
        let mut ctx = PluginContext::new("test-node".to_string());

        // Initially no stakes
        assert_eq!(plugin.engine.total_stake(), 0);

        // Sync stake from token plugin
        let sync = PluginEvent::Custom {
            source_plugin: "helm-token".to_string(),
            target_plugin: PLUGIN_NAME.to_string(),
            event_type: EVENT_STAKE_SYNC.to_string(),
            payload: serde_json::json!({
                "voter": "did:helm:agent-0001",
                "power": 1000_u64,
            }),
        };
        plugin.on_event(&mut ctx, &sync).await.unwrap();

        assert_eq!(plugin.engine.get_stake("did:helm:agent-0001"), 1000);
        assert_eq!(plugin.engine.total_stake(), 1000);
    }

    #[tokio::test]
    async fn plugin_stake_sync_remove() {
        let mut plugin = GovernancePlugin::with_defaults();
        let mut ctx = PluginContext::new("test-node".to_string());

        // Set initial stake
        plugin.engine.set_stake("did:helm:abc", 500);
        assert_eq!(plugin.engine.total_stake(), 500);

        // Sync with power=0 should remove
        let sync = PluginEvent::Custom {
            source_plugin: "helm-token".to_string(),
            target_plugin: PLUGIN_NAME.to_string(),
            event_type: EVENT_STAKE_SYNC.to_string(),
            payload: serde_json::json!({
                "voter": "did:helm:abc",
                "power": 0_u64,
            }),
        };
        plugin.on_event(&mut ctx, &sync).await.unwrap();

        assert_eq!(plugin.engine.get_stake("did:helm:abc"), 0);
        assert_eq!(plugin.engine.total_stake(), 0);
    }

    #[tokio::test]
    async fn plugin_ignores_other_targets() {
        let mut plugin = GovernancePlugin::with_defaults();
        let mut ctx = PluginContext::new("test-node".to_string());

        let event = PluginEvent::Custom {
            source_plugin: "test".to_string(),
            target_plugin: "helm-token".to_string(),
            event_type: "something".to_string(),
            payload: serde_json::json!({}),
        };
        plugin.on_event(&mut ctx, &event).await.unwrap();
        assert_eq!(plugin.registry.total(), 0);
    }
}
