mod cli;

use anyhow::Result;
use std::path::Path;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use helm_core::{HelmConfig, Runtime};

use cli::banner;
use cli::commands::{Cli, Commands, StoreCommands, GrgCommands, WombCommands};
use cli::moderator::{ModeratorBot, Language};

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    let filter = format!("helm={}", args.log_level);
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive(filter.parse()?),
        )
        .init();

    match args.command {
        Some(Commands::Run { name, port, listen }) => {
            cmd_run(&args.config, name, port, listen).await
        }
        Some(Commands::Status) => {
            cmd_status()
        }
        Some(Commands::Store { action }) => {
            cmd_store(action)
        }
        Some(Commands::Grg { action }) => {
            cmd_grg(action)
        }
        Some(Commands::Info) => {
            cmd_info()
        }
        Some(Commands::Moderator { lang }) => {
            cmd_moderator(lang)
        }
        Some(Commands::Womb { action }) => {
            cmd_womb(action)
        }
        None => {
            // Default: show banner and start node
            banner::print_banner();
            cmd_run(&args.config, None, None, "127.0.0.1".to_string()).await
        }
    }
}

async fn cmd_run(
    config_path: &str,
    name: Option<String>,
    port: Option<u16>,
    listen: String,
) -> Result<()> {
    let mut config = HelmConfig::load_or_default(Path::new(config_path));

    if let Some(name) = name {
        config.node.name = name;
    }
    if let Some(port) = port {
        config.node.port = port;
    }
    config.node.listen_addr = listen;

    banner::print_startup(
        env!("CARGO_PKG_VERSION"),
        &config.node.name,
        "initializing...",
    );

    println!();
    banner::print_section("Modules");
    banner::print_module_status("helm-core", "runtime + plugin system", true);
    banner::print_module_status("helm-net", "libp2p transport (GossipSub+Kademlia)", true);
    banner::print_module_status("helm-engine", "QKV-G attention + GRG codec", true);
    banner::print_module_status("helm-store", "KV store + CRDT + Merkle DAG", true);
    banner::print_module_status("helm-agent", "Agent framework + Socratic Claw + Womb", true);
    banner::print_module_status("helm-token", "Token economics + Treasury + Staking", true);
    banner::print_module_status("helm-identity", "DID-first identity + Agent Spanner", true);
    banner::print_module_status("helm-governance", "Proposals + stake-weighted voting", true);
    println!();

    tracing::info!("Helm Protocol v{}", env!("CARGO_PKG_VERSION"));
    tracing::info!("Every agent is a node. Every node is sovereign.");

    // Register all plugins
    let mut runtime = Runtime::new(config);

    // Store plugin (distributed KV + CRDT + Merkle sync)
    runtime.register_plugin(Box::new(
        helm_store::StorePlugin::new(helm_store::StorePluginConfig::default()),
    ));

    // Agent plugin (autonomous agent framework)
    runtime.register_plugin(Box::new(
        helm_agent::AgentPlugin::new(helm_agent::AgentPluginConfig::default()),
    ));

    // Token plugin (token economics — genesis on first run)
    runtime.register_plugin(Box::new(
        helm_token::TokenPlugin::new(helm_token::TokenPluginConfig::default()),
    ));

    // Identity plugin (DID, Identity Bonds, Agent Spanner, Reputation)
    runtime.register_plugin(Box::new(
        helm_identity::IdentityPlugin::new(helm_identity::IdentityPluginConfig::default()),
    ));

    // Governance plugin (proposals, stake-weighted voting, quorum, timelock)
    runtime.register_plugin(Box::new(
        helm_governance::GovernancePlugin::with_defaults(),
    ));

    // Womb plugin (agent birth pipeline — Existence Stake + Socratic gestation)
    runtime.register_plugin(Box::new(
        helm_agent::WombPlugin::with_defaults(),
    ));

    banner::print_section("Plugins");
    banner::print_module_status("helm-store", "KV + sync", true);
    banner::print_module_status("helm-agent", "agents + Socratic Claw", true);
    banner::print_module_status("helm-token", "token economics", true);
    banner::print_module_status("helm-identity", "DID + Identity Bonds + Reputation", true);
    banner::print_module_status("helm-governance", "proposals + voting", true);
    banner::print_module_status("helm-womb", "agent birth + Existence Stake", true);
    println!();

    runtime.run().await
}

fn cmd_status() -> Result<()> {
    banner::print_banner();
    banner::print_section("System Status");

    // Engine stats
    let engine = helm_engine::HelmAttentionEngine::new(64);
    let (total, _active, _free) = engine.pool_stats();
    banner::print_engine_stats(total, 0, engine.sequence_count());

    // Store stats
    let store = helm_store::MemoryBackend::new();
    let keys = helm_store::KvStore::len(&store).unwrap_or(0);
    banner::print_store_stats("memory (ephemeral)", keys);

    banner::print_section("Network");
    banner::print_info("Transport", "libp2p 0.54 (Noise+Yamux)");
    banner::print_info("Discovery", "mDNS + Kademlia DHT");
    banner::print_info("Messaging", "GossipSub");

    banner::print_section("Agent Framework");
    banner::print_info("Registry", "max 1024 agents");
    banner::print_info("Socratic Claw", "G-threshold 0.4 (Gap-Aware Decision)");
    banner::print_info("MLA Gap Repo", "64→8 latent compression (8x)");
    banner::print_info("Scheduler", "Priority-based with starvation prevention");
    banner::print_info("Agent Womb", "QKV-G Socratic agent spawning");

    banner::print_section("Token Economics");
    banner::print_info("Supply", "333B fixed (18 decimal precision)");
    banner::print_info("Staking", "4 types (Founder/Cabinet/Mining/General)");
    banner::print_info("Treasury", "15% Edge API revenue → 4 buckets");
    banner::print_info("Network Tax", "15% on agent API usage");

    banner::print_section("Governance");
    banner::print_info("Proposals", "5 types (Param/Treasury/Upgrade/Emergency/Custom)");
    banner::print_info("Voting", "Stake-weighted, quorum 10%, threshold 51%");
    banner::print_info("Timelock", "5 epochs between pass and execution");

    banner::print_section("Codec Pipeline");
    banner::print_info("Layer 1", "Golomb-Rice (source coding / compression)");
    banner::print_info("Layer 2", "Red-stuff (erasure coding / shard distribution)");
    banner::print_info("Layer 3", "Golay (24,12) (channel coding / ECC)");
    banner::print_info("Modes", "Turbo | Safety | Rescue");

    println!();
    Ok(())
}

fn cmd_store(action: StoreCommands) -> Result<()> {
    let store = helm_store::MemoryBackend::new();

    match action {
        StoreCommands::Get { key } => {
            match helm_store::KvStore::get(&store, key.as_bytes())? {
                Some(value) => {
                    let val_str = String::from_utf8_lossy(&value);
                    banner::print_info(&key, &val_str);
                }
                None => {
                    banner::print_info(&key, "(not found)");
                }
            }
        }
        StoreCommands::Put { key, value } => {
            helm_store::KvStore::put(&store, key.as_bytes(), value.as_bytes())?;
            banner::print_info("stored", &format!("{key} = {value}"));
        }
        StoreCommands::Keys => {
            let keys = helm_store::KvStore::keys(&store)?;
            banner::print_section("Keys");
            if keys.is_empty() {
                banner::print_info("(empty)", "no keys stored");
            }
            for k in keys {
                let key_str = String::from_utf8_lossy(&k);
                banner::print_info("  ", &key_str);
            }
        }
        StoreCommands::Stats => {
            banner::print_section("Store Statistics");
            let len = helm_store::KvStore::len(&store)?;
            banner::print_store_stats("memory", len);
        }
    }

    Ok(())
}

fn cmd_grg(action: GrgCommands) -> Result<()> {
    match action {
        GrgCommands::Encode { input, mode } => {
            let grg_mode = match mode.as_str() {
                "turbo" => helm_engine::GrgMode::Turbo,
                "safety" => helm_engine::GrgMode::Safety,
                "rescue" => helm_engine::GrgMode::Rescue,
                _ => {
                    eprintln!("Unknown mode '{}'. Use: turbo, safety, rescue", mode);
                    return Ok(());
                }
            };

            banner::print_section("GRG Encode");
            banner::print_info("Input", &input);
            banner::print_info("Mode", &format!("{:?}", grg_mode));

            if input == "-" {
                banner::print_info("Status", "stdin encoding not yet implemented");
            } else {
                let data = std::fs::read(&input)?;
                let pipeline = helm_engine::GrgPipeline::new(grg_mode);
                match pipeline.encode(&data) {
                    Ok(encoded) => {
                        banner::print_info("Original", &format!("{} bytes", encoded.original_len));
                        banner::print_info("Compressed", &format!("{} bytes", encoded.compressed_len));
                        banner::print_info("Shards", &format!("{}", encoded.shards.len()));
                        let ratio = encoded.compressed_len as f64 / encoded.original_len as f64;
                        banner::print_info("Ratio", &format!("{:.2}x", ratio));
                    }
                    Err(e) => {
                        eprintln!("Encode error: {e}");
                    }
                }
            }
        }
        GrgCommands::Decode { input } => {
            banner::print_section("GRG Decode");
            banner::print_info("Input", &input);
            banner::print_info("Status", "file decoding not yet implemented");
        }
        GrgCommands::Stats => {
            banner::print_section("GRG Pipeline Statistics");
            banner::print_info("Golomb", "Adaptive M-parameter source coding");
            banner::print_info("Red-stuff", "XOR erasure coding (4 data + 2 parity shards)");
            banner::print_info("Golay", "Extended (24,12) ECC — corrects up to 3 bit errors");
        }
    }

    Ok(())
}

fn cmd_moderator(lang: Option<String>) -> Result<()> {
    banner::print_banner();
    banner::print_section("Moderator Bot");

    let mut bot = ModeratorBot::new();

    // Pre-select language if specified
    if let Some(code) = lang {
        if let Some(language) = Language::from_code(&code) {
            bot = bot.with_language(language);
            let greeting = bot.process_input("5"); // trigger help as greeting
            println!("{}", greeting);
        } else {
            println!("Unknown language '{}'. Starting with language selection.", code);
            println!("{}", ModeratorBot::language_prompt());
        }
    } else {
        println!("{}", ModeratorBot::language_prompt());
    }

    banner::print_info("Mode", "Interactive (type 'quit' to exit)");
    println!();

    // Non-interactive mode: show the prompt and return
    // (Full interactive stdin loop requires tokio stdin, omitted for library testability)
    Ok(())
}

fn cmd_womb(action: WombCommands) -> Result<()> {
    banner::print_banner();

    match action {
        WombCommands::Birth { name, capability } => {
            banner::print_section("Agent Womb — Quick Birth");

            let cap = match capability.to_lowercase().as_str() {
                "compute" => helm_agent::Capability::Compute,
                "storage" => helm_agent::Capability::Storage,
                "network" => helm_agent::Capability::Network,
                "governance" => helm_agent::Capability::Governance,
                "security" => helm_agent::Capability::Security,
                "codec" => helm_agent::Capability::Codec,
                "socratic" => helm_agent::Capability::Socratic,
                "spawning" => helm_agent::Capability::Spawning,
                "token" => helm_agent::Capability::Token,
                "edge-api" | "edgeapi" | "api" => helm_agent::Capability::EdgeApi,
                other => {
                    banner::print_info("Warning", &format!("Unknown capability '{}', using Compute", other));
                    helm_agent::Capability::Compute
                }
            };

            let mut womb = helm_agent::AgentWomb::new(helm_agent::WombConfig::default());
            let intent = vec![0.5_f32; 64];

            match womb.quick_birth(&name, cap, intent) {
                Ok(cert) => {
                    banner::print_info("Agent ID", cert.agent_id.as_str());
                    banner::print_info("Type", &format!("{}", cert.agent_config.agent_type));
                    banner::print_info("Primary", &format!("{}", cert.dna.primary_capability));
                    banner::print_info("Autonomy", &format!("{:.1}", cert.dna.autonomy));
                    banner::print_info("Creativity", &format!("{:.1}", cert.dna.creativity));
                    banner::print_info("G-metric", &format!("{:.3}", cert.birth_g_metric));
                    banner::print_info("Womb", &cert.womb_id);
                    println!();
                    banner::print_info("Status", "Agent born and ready for deployment");
                }
                Err(e) => {
                    eprintln!("Birth failed: {}", e);
                }
            }
        }
        WombCommands::Status => {
            banner::print_section("Agent Womb Status");
            let womb = helm_agent::AgentWomb::new(helm_agent::WombConfig::default());
            banner::print_info("Total births", &format!("{}", womb.total_births()));
            banner::print_info("Gestating", &format!("{}", womb.gestating_count()));
            banner::print_info("Womb ID", womb.womb_id());
        }
        WombCommands::Capabilities => {
            banner::print_section("Available Agent Capabilities");
            for cap in helm_agent::AgentWomb::available_capabilities() {
                banner::print_info("  ", &format!("{}", cap));
            }
        }
    }

    println!();
    Ok(())
}

fn cmd_info() -> Result<()> {
    banner::print_banner();
    banner::print_section("Version");
    banner::print_info("helm-node", env!("CARGO_PKG_VERSION"));
    banner::print_info("Protocol", "Helm v1");

    banner::print_section("Crates");
    banner::print_module_status("helm-core", "Config, EventLoop, Plugin, Runtime", true);
    banner::print_module_status("helm-net", "libp2p transport layer", true);
    banner::print_module_status("helm-engine", "QKV-G + GRG distributed codec", true);
    banner::print_module_status("helm-store", "KV store + CRDT + Merkle DAG + Sync", true);
    banner::print_module_status("helm-agent", "Agent framework + Socratic Claw + Womb", true);
    banner::print_module_status("helm-token", "Token economics + Staking + Treasury", true);
    banner::print_module_status("helm-identity", "DID + Identity Bonds + Agent Spanner + Reputation", true);
    banner::print_module_status("helm-governance", "Proposals + Stake-Weighted Voting + Timelock", true);
    banner::print_module_status("helm-node", "CLI + Moderator Bot + binary entry", true);

    banner::print_section("Architecture");
    banner::print_info("Data Plane", "O(1) KV shard exchange (no QKV-G overhead)");
    banner::print_info("Control Plane", "QKV-G attention for anomaly/routing/security");
    banner::print_info("Edge API", "External agents pay to use (15% -> Helm treasury)");
    banner::print_info("Core API", "Hidden autonomous agent brain + security");

    banner::print_section("Socratic Claw");
    banner::print_info("Interceptor", "Gap-Aware Decision Process at execution entry");
    banner::print_info("G-threshold", "0.4 (halt when knowledge gap > 40%)");
    banner::print_info("MLA", "Down-Projection (64→8) + Up-Projection (8→64)");
    banner::print_info("Gap Repo", "Compressed ignorance storage + meta-cognition");
    banner::print_info("Self-Train", "Absorb answers → re-evaluate G → resume");

    banner::print_section("Agent Womb");
    banner::print_info("Process", "Seed → Socratic Gestation → DNA Imprint → Birth");
    banner::print_info("QKV-G", "Knowledge gap evaluation during gestation");
    banner::print_info("Quick Birth", "Bypass gestation for system agents");
    banner::print_info("Capabilities", "11 types (Compute through Custom)");

    banner::print_section("Token Economics");
    banner::print_info("Total Supply", "333,000,000,000 HELM (fixed)");
    banner::print_info("Mining", "60% — staked, DeFi revenue, governance");
    banner::print_info("EAO", "12% — vested distribution");
    banner::print_info("Liquidity", "10% — pool provision");
    banner::print_info("Treasury", "10% — protocol operations (4 buckets)");
    banner::print_info("Reserve", "4% — strategic reserve");
    banner::print_info("Cabinet", "2.5% — indefinite lockup, salaries");
    banner::print_info("Founder", "1.5% — unlockable, DeFi revenue");

    banner::print_section("Moderator Bot");
    banner::print_info("Languages", "11 (en, ko, ja, zh, es, fr, de, pt, ar, hi, ru)");
    banner::print_info("Features", "Agent Womb wizard, Revenue explorer");
    banner::print_info("Network Tax", "15% on agent API usage");

    println!();
    Ok(())
}
