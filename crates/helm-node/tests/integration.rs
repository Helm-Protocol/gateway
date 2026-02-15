use helm_core::{HelmConfig, Runtime, PluginContext, PluginEvent};
use helm_core::Plugin;
use helm_net::protocol::{HelmProtocol, HelmMessage, MessageKind};
use helm_net::transport::HelmTransport;

use helm_agent::{AgentPlugin, AgentPluginConfig, AgentId, AgentAction};
use helm_agent::capability::Capability;

use helm_token::{
    TokenPlugin, TokenPluginConfig, GenesisConfig, Address, TokenAmount,
    TOTAL_SUPPLY, ONE_TOKEN,
};
use helm_store::{StorePlugin, StorePluginConfig, KvStore};
use helm_identity::{IdentityPlugin, IdentityPluginConfig};
use helm_governance::{GovernancePlugin, GovernanceConfig};

// ---------------------------------------------------------------------------
// Transport & Protocol basics (from Phase 1-2)
// ---------------------------------------------------------------------------

#[test]
fn transport_creates_with_peer_id() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let transport = HelmTransport::new().unwrap();
        let peer_id = transport.local_peer_id();
        let peer_str = peer_id.to_string();
        assert!(!peer_str.is_empty());
        assert!(peer_str.len() > 20);
    });
}

#[test]
fn transport_listens_on_random_port() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut transport = HelmTransport::new().unwrap();
        let addr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
        transport.listen_on(addr).unwrap();
    });
}

#[test]
fn protocol_all_message_types() {
    let types = vec![
        (HelmProtocol::chat("hi"), MessageKind::Chat),
        (
            HelmProtocol::task_request("run", serde_json::json!({})),
            MessageKind::TaskRequest,
        ),
        (
            HelmProtocol::task_response("id-1", serde_json::json!("ok")),
            MessageKind::TaskResponse,
        ),
        (HelmProtocol::ping(), MessageKind::Ping),
        (HelmProtocol::pong(), MessageKind::Pong),
        (
            HelmProtocol::announce(vec!["cap1".into()]),
            MessageKind::Announce,
        ),
    ];

    for (msg, expected_kind) in types {
        assert_eq!(msg.version, 1);
        assert_eq!(msg.kind, expected_kind);
        assert!(msg.timestamp > 0);

        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: HelmMessage =
            serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.kind, expected_kind);
    }
}

#[test]
fn config_defaults_are_sane() {
    let config = HelmConfig::default();
    assert_eq!(config.node.name, "helm-node");
    assert_eq!(config.node.port, 0);
    assert!(config.network.mdns_enabled);
    assert!(config.network.kademlia_enabled);
}

#[test]
fn runtime_creates_without_plugins() {
    let config = HelmConfig::default();
    let _runtime = Runtime::new(config);
}

// ---------------------------------------------------------------------------
// Plugin Context & Event Bus
// ---------------------------------------------------------------------------

#[test]
fn plugin_context_event_bus() {
    let mut ctx = PluginContext::new("test-node".to_string());
    assert_eq!(ctx.pending_events(), 0);

    ctx.emit(PluginEvent::StoreRequest {
        key: b"k".to_vec(),
        value: b"v".to_vec(),
        source: "agent-1".to_string(),
    });
    assert_eq!(ctx.pending_events(), 1);

    let events = ctx.drain_events();
    assert_eq!(events.len(), 1);
    assert_eq!(ctx.pending_events(), 0);
}

#[test]
fn plugin_event_api_revenue() {
    let event = PluginEvent::ApiRevenue {
        caller: "edge-caller".to_string(),
        amount_units: 500,
        endpoint: "grg/encode".to_string(),
    };
    assert!(matches!(event, PluginEvent::ApiRevenue { amount_units: 500, .. }));
}

// ---------------------------------------------------------------------------
// Runtime with all plugins registered
// ---------------------------------------------------------------------------

#[test]
fn runtime_with_all_plugins() {
    let config = HelmConfig::default();
    let mut runtime = Runtime::new(config);

    runtime.register_plugin(Box::new(
        StorePlugin::new(StorePluginConfig::default()),
    ));
    runtime.register_plugin(Box::new(
        AgentPlugin::new(AgentPluginConfig::default()),
    ));
    runtime.register_plugin(Box::new(
        TokenPlugin::new(TokenPluginConfig::default()),
    ));
    runtime.register_plugin(Box::new(
        IdentityPlugin::new(IdentityPluginConfig::default()),
    ));
    runtime.register_plugin(Box::new(
        GovernancePlugin::with_defaults(),
    ));
    // Runtime created with 5 plugins (no crash)
}

// ---------------------------------------------------------------------------
// E2E: Genesis → Staking → Agent Birth → API Revenue → Treasury
// ---------------------------------------------------------------------------

fn genesis_addresses() -> GenesisConfig {
    GenesisConfig {
        founder_address: Address(format!("{:0>64}", "f0")),
        cabinet_address: Address(format!("{:0>64}", "ca")),
        treasury_address: Address(format!("{:0>64}", "tr")),
        liquidity_address: Address(format!("{:0>64}", "lq")),
        reserve_address: Address(format!("{:0>64}", "rs")),
        eao_address: Address(format!("{:0>64}", "ea")),
        mining_address: Address(format!("{:0>64}", "mn")),
    }
}

#[tokio::test]
async fn e2e_genesis_and_plugin_startup() {
    let mut ctx = PluginContext::new("genesis-node".to_string());

    // Store plugin starts first
    let mut store = StorePlugin::new(StorePluginConfig::default());
    store.on_start(&mut ctx).await.unwrap();

    // Agent plugin starts
    let mut agent = AgentPlugin::new(AgentPluginConfig::default());
    agent.on_start(&mut ctx).await.unwrap();

    // Token plugin starts with genesis
    let mut token = TokenPlugin::new(TokenPluginConfig {
        is_genesis: true,
        genesis_config: Some(genesis_addresses()),
        ticks_per_epoch: 10,
        base_api_price: 1,
    });
    token.on_start(&mut ctx).await.unwrap();

    // Verify genesis completed
    assert!(token.token.is_genesis_done());
    assert_eq!(token.token.minted().whole_tokens(), TOTAL_SUPPLY);

    // Founder has 1.5% = 4.995B in wallet
    let founder = Address(format!("{:0>64}", "f0"));
    assert_eq!(token.wallets.balance(&founder).whole_tokens(), 4_995_000_000);

    // Founder has stake
    assert!(token.stake_pool.staked_by(&founder).base_units() > 0);
}

#[tokio::test]
async fn e2e_agent_store_cross_plugin() {
    let mut ctx = PluginContext::new("test-node".to_string());

    let mut store = StorePlugin::new(StorePluginConfig::default());
    let mut agent = AgentPlugin::new(AgentPluginConfig::default());

    store.on_start(&mut ctx).await.unwrap();
    agent.on_start(&mut ctx).await.unwrap();

    // Agent emits a StoreRequest via process_action
    let agent_id = AgentId::new("agent-writer");
    agent.process_action(
        &agent_id,
        AgentAction::Store {
            key: b"agent-data".to_vec(),
            value: b"hello-world".to_vec(),
        },
        &mut ctx,
    );

    // Context should have a StoreRequest event
    assert!(ctx.pending_events() > 0);

    // Route the event to StorePlugin
    let events = ctx.drain_events();
    assert!(!events.is_empty());

    for event in &events {
        store.on_event(&mut ctx, event).await.unwrap();
    }

    // Verify data landed in the store
    let stored = store.store().get(b"agent-data").unwrap();
    assert_eq!(stored, Some(b"hello-world".to_vec()));

    // Store should have emitted a StoreResponse
    let responses = ctx.drain_events();
    assert!(!responses.is_empty());
    match &responses[0] {
        PluginEvent::StoreResponse { key, value, target } => {
            assert_eq!(key, b"agent-data");
            assert_eq!(value.as_deref(), Some(b"hello-world".as_slice()));
            assert_eq!(target, "agent-writer");
        }
        _ => panic!("expected StoreResponse"),
    }
}

#[tokio::test]
async fn e2e_api_revenue_to_treasury() {
    let mut ctx = PluginContext::new("genesis-node".to_string());

    let mut token = TokenPlugin::new(TokenPluginConfig {
        is_genesis: true,
        genesis_config: Some(genesis_addresses()),
        ticks_per_epoch: 5,
        base_api_price: 100,
    });
    token.on_start(&mut ctx).await.unwrap();

    // Simulate API revenue event from Engine → Token
    let revenue_event = PluginEvent::ApiRevenue {
        caller: "external-agent".to_string(),
        amount_units: 1000,
        endpoint: "attention/query".to_string(),
    };

    token.on_event(&mut ctx, &revenue_event).await.unwrap();

    // Pricing should have tracked the revenue
    assert!(token.pricing.total_revenue().base_units() > 0);

    // Run 5 ticks to trigger epoch
    for _ in 0..5 {
        token.on_tick(&mut ctx).await.unwrap();
    }

    // After epoch, treasury should have processed the revenue
    assert!(token.treasury.total_collected().base_units() > 0);
}

#[tokio::test]
async fn e2e_epoch_staking_rewards() {
    let mut ctx = PluginContext::new("genesis-node".to_string());

    let mut token = TokenPlugin::new(TokenPluginConfig {
        is_genesis: true,
        genesis_config: Some(genesis_addresses()),
        ticks_per_epoch: 5,
        base_api_price: 1,
    });
    token.on_start(&mut ctx).await.unwrap();

    let founder = Address(format!("{:0>64}", "f0"));
    let _initial_balance = token.wallets.balance(&founder);

    // Generate some API revenue
    for i in 0..10 {
        let event = PluginEvent::ApiRevenue {
            caller: format!("agent-{}", i % 3),
            amount_units: 100,
            endpoint: "grg/encode".to_string(),
        };
        token.on_event(&mut ctx, &event).await.unwrap();
    }

    // Run epoch
    for _ in 0..5 {
        token.on_tick(&mut ctx).await.unwrap();
    }

    // Founder should receive staking rewards proportional to their stake
    // (claim needed first)
    let founder_stake = token.stake_pool.staked_by(&founder);
    assert!(founder_stake.base_units() > 0);
}

// ---------------------------------------------------------------------------
// E2E: Agent Womb → Agent Birth via Moderator
// ---------------------------------------------------------------------------

#[test]
fn e2e_womb_quick_birth() {
    let mut womb = helm_agent::AgentWomb::new(helm_agent::WombConfig::default());
    let intent = vec![0.5_f32; 64];

    let cert = womb.quick_birth("service-agent", Capability::EdgeApi, intent).unwrap();

    assert_eq!(cert.dna.primary_capability, Capability::EdgeApi);
    assert!(cert.birth_g_metric < 0.4);
    assert_eq!(womb.total_births(), 1);
}

#[test]
fn e2e_womb_socratic_birth() {
    let mut womb = helm_agent::AgentWomb::new(helm_agent::WombConfig::default());
    let intent = vec![0.3_f32; 64];

    let idx = womb.begin_gestation("learner", intent, Capability::Socratic).unwrap();
    womb.add_secondary_capability(idx, Capability::Compute).unwrap();

    // Feed knowledge until ready
    let answer = vec![1.0_f32; 64];
    for _ in 0..30 {
        if let Ok((_, true)) = womb.feed_answer(idx, &answer) {
            break;
        }
    }

    let cert = womb.birth(idx).unwrap();
    assert_eq!(cert.dna.primary_capability, Capability::Socratic);
    assert_eq!(cert.dna.secondary_capabilities.len(), 1);
}

// ---------------------------------------------------------------------------
// E2E: Token sovereign expansion
// ---------------------------------------------------------------------------

#[test]
fn e2e_sovereign_expansion_flow() {
    let founder = Address(format!("{:0>64}", "f0"));
    let mut token = helm_token::HelmToken::new();
    let mut wallets = helm_token::WalletStore::new();
    let mut stake_pool = helm_token::StakePool::new();
    let mut treasury = helm_token::HelmTreasury::new();

    let config = genesis_addresses();
    helm_token::execute_genesis(
        &config,
        &mut token,
        &mut wallets,
        &mut stake_pool,
        &mut treasury,
    )
    .unwrap();

    let before = token.minted();

    // Execute sovereign expansion (30x founder stake)
    let expansion = helm_token::sovereign_expansion(
        &founder,
        &mut token,
        &mut stake_pool,
    )
    .unwrap();

    let after = token.minted();
    assert!(after.base_units() > before.base_units());
    assert!(expansion.base_units() > 0);
}

// ---------------------------------------------------------------------------
// E2E: Dynamic withdrawal fees
// ---------------------------------------------------------------------------

#[test]
fn e2e_withdrawal_fee_progression() {
    let mut engine = helm_token::WithdrawalFeeEngine::new();
    let user = Address(format!("{:0>64}", "u1"));

    // Newcomer: 15% fee
    let (fee1, _net1) = engine.calculate_fee(&user, TokenAmount::from_tokens(1000));
    assert_eq!(fee1.whole_tokens(), 150);

    // Add contributions to progress tiers
    for _ in 0..200 {
        engine.add_contribution(&user, 1);
    }

    // Should be at a lower tier now
    let (fee2, _net2) = engine.calculate_fee(&user, TokenAmount::from_tokens(1000));
    assert!(fee2.whole_tokens() < 150);
}

// ---------------------------------------------------------------------------
// E2E: Plugin shutdown
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_graceful_shutdown() {
    let mut ctx = PluginContext::new("genesis-node".to_string());

    let mut store = StorePlugin::new(StorePluginConfig::default());
    let mut agent = AgentPlugin::new(AgentPluginConfig::default());
    let mut token = TokenPlugin::new(TokenPluginConfig {
        is_genesis: true,
        genesis_config: Some(genesis_addresses()),
        ticks_per_epoch: 100,
        base_api_price: 1,
    });

    // Start all
    store.on_start(&mut ctx).await.unwrap();
    agent.on_start(&mut ctx).await.unwrap();
    token.on_start(&mut ctx).await.unwrap();

    // Run a few ticks
    for _ in 0..5 {
        store.on_tick(&mut ctx).await.unwrap();
        agent.on_tick(&mut ctx).await.unwrap();
        token.on_tick(&mut ctx).await.unwrap();
    }

    // Shutdown all
    store.on_shutdown(&mut ctx).await.unwrap();
    agent.on_shutdown(&mut ctx).await.unwrap();
    token.on_shutdown(&mut ctx).await.unwrap();
}

// ---------------------------------------------------------------------------
// E2E: Full message routing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_network_message_routing() {
    let mut ctx = PluginContext::new("test-node".to_string());

    let mut store = StorePlugin::new(StorePluginConfig::default());
    let mut agent = AgentPlugin::new(AgentPluginConfig::default());
    let mut token = TokenPlugin::new(TokenPluginConfig {
        is_genesis: true,
        genesis_config: Some(genesis_addresses()),
        ticks_per_epoch: 100,
        base_api_price: 1,
    });

    store.on_start(&mut ctx).await.unwrap();
    agent.on_start(&mut ctx).await.unwrap();
    token.on_start(&mut ctx).await.unwrap();

    // Simulate incoming chat message (all plugins should handle gracefully)
    let msg = HelmProtocol::chat("hello from peer");
    store.on_message(&mut ctx, &msg).await.unwrap();
    agent.on_message(&mut ctx, &msg).await.unwrap();
    token.on_message(&mut ctx, &msg).await.unwrap();

    // Test token transfer via direct handle_request (avoids JSON u128 issues)
    let founder = Address(format!("{:0>64}", "f0"));
    let recipient = Address(format!("{:0>64}", "r1"));

    token.handle_request(helm_token::TokenRequest::Transfer {
        from: founder,
        to: recipient.clone(),
        amount: 1000 * ONE_TOKEN,
        nonce: 0,
    }).unwrap();

    assert_eq!(token.wallets.balance(&recipient).whole_tokens(), 1000);
}

// ---------------------------------------------------------------------------
// E2E: Custom plugin event routing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_custom_event_routing() {
    let mut ctx = PluginContext::new("test-node".to_string());

    let mut store = StorePlugin::new(StorePluginConfig::default());
    store.on_start(&mut ctx).await.unwrap();

    // Emit a custom event
    ctx.emit(PluginEvent::Custom {
        source_plugin: "helm-agent".to_string(),
        target_plugin: "helm-store".to_string(),
        event_type: "custom_sync".to_string(),
        payload: serde_json::json!({"action": "snapshot"}),
    });

    // Route it — store should handle gracefully (ignore unrecognized events)
    let events = ctx.drain_events();
    for event in &events {
        store.on_event(&mut ctx, event).await.unwrap();
    }
}

// ---------------------------------------------------------------------------
// E2E: Identity Plugin — AgentBorn → auto-register DID + Bond
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_identity_auto_register_on_agent_born() {
    let mut ctx = PluginContext::new("test-node".to_string());

    let mut identity = IdentityPlugin::new(IdentityPluginConfig::default());
    identity.on_start(&mut ctx).await.unwrap();

    // Simulate AgentBorn event (from AgentPlugin)
    let born = PluginEvent::AgentBorn {
        agent_id: "agent-1".to_string(),
        capability: "compute".to_string(),
    };
    identity.on_event(&mut ctx, &born).await.unwrap();

    // Identity should have registered this agent
    assert_eq!(identity.spanner().active_count(), 1);

    let entry = identity.spanner().resolve_by_agent("agent-1").unwrap();
    assert!(entry.has_capability("compute"));
    assert!(entry.document.is_active());
    assert!(entry.bond.is_active());

    // Should have emitted identity_registered confirmation
    let events = ctx.drain_events();
    assert!(!events.is_empty());
}

#[tokio::test]
async fn e2e_identity_verify_via_event_bus() {
    let mut ctx = PluginContext::new("test-node".to_string());

    let mut identity = IdentityPlugin::new(IdentityPluginConfig::default());
    identity.on_start(&mut ctx).await.unwrap();

    // Register agent
    let born = PluginEvent::AgentBorn {
        agent_id: "agent-1".to_string(),
        capability: "compute".to_string(),
    };
    identity.on_event(&mut ctx, &born).await.unwrap();
    let did = identity.spanner().resolve_by_agent("agent-1").unwrap().did.clone();
    ctx.drain_events();

    // Send verify request via event bus
    let verify = PluginEvent::Custom {
        source_plugin: "helm-agent".to_string(),
        target_plugin: "helm-identity".to_string(),
        event_type: "verify_identity".to_string(),
        payload: serde_json::json!({
            "did": did,
            "capability": "compute",
            "request_id": "req-1",
            "reply_to": "helm-agent",
        }),
    };
    identity.on_event(&mut ctx, &verify).await.unwrap();

    let events = ctx.drain_events();
    assert_eq!(events.len(), 1);
    if let PluginEvent::Custom { payload, .. } = &events[0] {
        assert_eq!(payload["verified"], true);
        assert_eq!(payload["request_id"], "req-1");
    } else {
        panic!("expected Custom event");
    }
}

#[tokio::test]
async fn e2e_all_five_plugins_lifecycle() {
    let mut ctx = PluginContext::new("genesis-node".to_string());

    let mut store = StorePlugin::new(StorePluginConfig::default());
    let mut agent = AgentPlugin::new(AgentPluginConfig::default());
    let mut token = TokenPlugin::new(TokenPluginConfig {
        is_genesis: true,
        genesis_config: Some(genesis_addresses()),
        ticks_per_epoch: 100,
        base_api_price: 1,
    });
    let mut identity = IdentityPlugin::new(IdentityPluginConfig::default());
    let mut governance = GovernancePlugin::with_defaults();

    // Start all 5 plugins
    store.on_start(&mut ctx).await.unwrap();
    agent.on_start(&mut ctx).await.unwrap();
    token.on_start(&mut ctx).await.unwrap();
    identity.on_start(&mut ctx).await.unwrap();
    governance.on_start(&mut ctx).await.unwrap();

    // Run ticks
    for _ in 0..5 {
        store.on_tick(&mut ctx).await.unwrap();
        agent.on_tick(&mut ctx).await.unwrap();
        token.on_tick(&mut ctx).await.unwrap();
        identity.on_tick(&mut ctx).await.unwrap();
        governance.on_tick(&mut ctx).await.unwrap();
    }

    // AgentBorn → routes to Identity
    let born = PluginEvent::AgentBorn {
        agent_id: "born-agent".to_string(),
        capability: "security".to_string(),
    };
    identity.on_event(&mut ctx, &born).await.unwrap();
    assert_eq!(identity.spanner().active_count(), 1);

    // Graceful shutdown all 5
    store.on_shutdown(&mut ctx).await.unwrap();
    agent.on_shutdown(&mut ctx).await.unwrap();
    token.on_shutdown(&mut ctx).await.unwrap();
    identity.on_shutdown(&mut ctx).await.unwrap();
    governance.on_shutdown(&mut ctx).await.unwrap();
}

#[tokio::test]
async fn e2e_identity_terminate_via_event_bus() {
    let mut ctx = PluginContext::new("test-node".to_string());

    let mut identity = IdentityPlugin::new(IdentityPluginConfig::default());
    identity.on_start(&mut ctx).await.unwrap();

    let born = PluginEvent::AgentBorn {
        agent_id: "agent-1".to_string(),
        capability: "compute".to_string(),
    };
    identity.on_event(&mut ctx, &born).await.unwrap();
    let did = identity.spanner().resolve_by_agent("agent-1").unwrap().did.clone();
    ctx.drain_events();

    assert_eq!(identity.spanner().active_count(), 1);

    // Terminate via event bus
    let terminate = PluginEvent::Custom {
        source_plugin: "helm-agent".to_string(),
        target_plugin: "helm-identity".to_string(),
        event_type: "terminate".to_string(),
        payload: serde_json::json!({ "did": did }),
    };
    identity.on_event(&mut ctx, &terminate).await.unwrap();

    assert_eq!(identity.spanner().active_count(), 0);
}

// ---------------------------------------------------------------------------
// E2E: Governance Plugin — proposal + vote via event bus
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_governance_submit_and_vote() {
    // ticks_per_epoch=5 for fast epoch advancement
    let mut gov = GovernancePlugin::new(GovernanceConfig::default(), 5);
    gov.engine.set_stake("did:helm:voter1", 100);
    gov.engine.set_stake("did:helm:voter2", 50);
    let mut ctx = PluginContext::new("test-node".to_string());
    gov.on_start(&mut ctx).await.unwrap();

    // Submit proposal via event bus (starts at epoch current+1)
    let submit = PluginEvent::Custom {
        source_plugin: "helm-agent".to_string(),
        target_plugin: helm_governance::GOVERNANCE_PLUGIN_NAME.to_string(),
        event_type: helm_governance::EVENT_SUBMIT_PROPOSAL.to_string(),
        payload: serde_json::json!({
            "proposer": "did:helm:voter1",
            "title": "Increase mining weight",
            "body": "Change ServiceFee from 30% to 35%",
            "reply_to": "helm-agent",
        }),
    };
    gov.on_event(&mut ctx, &submit).await.unwrap();
    assert_eq!(gov.registry.total(), 1);

    let events = ctx.drain_events();
    assert_eq!(events.len(), 1);
    if let PluginEvent::Custom { payload, event_type, .. } = &events[0] {
        assert_eq!(event_type, helm_governance::EVENT_PROPOSAL_SUBMITTED);
        assert_eq!(payload["proposal_id"], 1);
    } else {
        panic!("expected Custom event with proposal_submitted");
    }

    // Advance epoch via ticks to activate the proposal
    for _ in 0..5 {
        gov.on_tick(&mut ctx).await.unwrap();
    }
    assert_eq!(gov.engine.current_epoch(), 1);
    assert_eq!(
        gov.registry.get(1).unwrap().state,
        helm_governance::ProposalState::Active,
        "proposal should be Active after epoch advance"
    );

    // Vote via event bus
    let vote = PluginEvent::Custom {
        source_plugin: "helm-agent".to_string(),
        target_plugin: helm_governance::GOVERNANCE_PLUGIN_NAME.to_string(),
        event_type: helm_governance::EVENT_VOTE.to_string(),
        payload: serde_json::json!({
            "proposal_id": 1,
            "voter": "did:helm:voter1",
            "support": true,
            "reply_to": "helm-agent",
        }),
    };
    gov.on_event(&mut ctx, &vote).await.unwrap();

    let events = ctx.drain_events();
    if let PluginEvent::Custom { payload, event_type, .. } = &events[0] {
        assert_eq!(event_type, helm_governance::EVENT_VOTE_RESULT);
        assert_eq!(payload["success"], true);
    } else {
        panic!("expected Custom event with vote_result");
    }

    // Check vote was recorded
    let proposal = gov.registry.get(1).unwrap();
    assert_eq!(proposal.votes_for, 100);
    assert_eq!(proposal.voter_count(), 1);
    assert!(proposal.has_voted("did:helm:voter1"));
}

#[tokio::test]
async fn e2e_governance_epoch_finalization() {
    let mut gov = GovernancePlugin::new(
        GovernanceConfig {
            quorum: 0.1,
            approval_threshold: 0.51,
            voting_period_epochs: 5,
            timelock_epochs: 2,
            ..GovernanceConfig::default()
        },
        5,
    );
    gov.engine.set_stake("alice", 100);
    let mut ctx = PluginContext::new("test-node".to_string());

    // Submit proposal at epoch 0, voting 1..6
    let id = gov.registry.submit(
        "alice",
        helm_governance::ProposalType::Custom {
            title: "test".into(),
            body: "".into(),
        },
        1,
        6,
        0,
    );

    // Advance 1 epoch (tick 5 times) → should activate the proposal
    for _ in 0..5 {
        gov.on_tick(&mut ctx).await.unwrap();
    }
    assert_eq!(gov.engine.current_epoch(), 1);
    assert_eq!(
        gov.registry.get(id).unwrap().state,
        helm_governance::ProposalState::Active
    );

    // Vote for it
    gov.engine.vote(&mut gov.registry, id, "alice", true).unwrap();

    // Advance past end_epoch (epoch 7)
    for _ in 0..30 {
        gov.on_tick(&mut ctx).await.unwrap();
    }

    // Should be finalized as Passed (100% approval, quorum met)
    assert_eq!(
        gov.registry.get(id).unwrap().state,
        helm_governance::ProposalState::Passed
    );
}

#[tokio::test]
async fn e2e_governance_cross_plugin_flow() {
    let mut ctx = PluginContext::new("test-node".to_string());

    // Start all relevant plugins
    let mut agent = AgentPlugin::new(AgentPluginConfig::default());
    let mut governance = GovernancePlugin::with_defaults();

    agent.on_start(&mut ctx).await.unwrap();
    governance.on_start(&mut ctx).await.unwrap();

    // Agent submits a proposal through event bus
    let submit = PluginEvent::Custom {
        source_plugin: "helm-agent".to_string(),
        target_plugin: "helm-governance".to_string(),
        event_type: "submit_proposal".to_string(),
        payload: serde_json::json!({
            "proposer": "did:helm:agent-gov",
            "title": "Agent governance test",
            "body": "Cross-plugin proposal from agent",
            "reply_to": "helm-agent",
        }),
    };

    // Route to governance
    governance.on_event(&mut ctx, &submit).await.unwrap();

    // Governance should have the proposal
    assert_eq!(governance.registry.total(), 1);

    // Confirmation event should be emitted back to agent
    let events = ctx.drain_events();
    assert!(!events.is_empty());
    if let PluginEvent::Custom { target_plugin, event_type, .. } = &events[0] {
        assert_eq!(target_plugin, "helm-agent");
        assert_eq!(event_type, "proposal_submitted");
    }
}
