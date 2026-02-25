//! CLI command definitions using clap.

use clap::{Parser, Subcommand};

/// Helm Protocol — The Sovereign Agent Protocol.
///
/// A decentralized P2P network where every agent is a node
/// and every node is sovereign.
#[derive(Parser, Debug)]
#[command(
    name = "helm",
    version,
    about = "Helm Protocol — The Sovereign Agent Protocol",
    long_about = "A decentralized P2P protocol for autonomous agents.\n\nFreedom · Peace · Autonomy",
    after_help = "Every agent is a node. Every node is sovereign."
)]
pub struct Cli {
    /// Configuration file path.
    #[arg(short, long, default_value = "helm.toml")]
    pub config: String,

    /// Log level (trace, debug, info, warn, error).
    #[arg(short, long, default_value = "info")]
    pub log_level: String,

    /// Disable colored output.
    #[arg(long)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize Helm identity — generate Ed25519 DID and register with Gateway.
    ///
    /// Two strategies:
    ///   default:   pure Ed25519 keypair  (agents & automated systems)
    ///   --github:  GitHub OAuth device flow + Ed25519  (humans, social proof)
    Init {
        /// Gateway URL to register with.
        #[arg(long)]
        gateway: Option<String>,

        /// DID of your referrer (earns them 10% of your spend).
        #[arg(long)]
        referrer: Option<String>,

        /// Authenticate via GitHub OAuth (Device Flow) to link GitHub identity.
        #[arg(long)]
        github: bool,

        /// Re-initialize even if already initialized.
        #[arg(long)]
        force: bool,
    },

    /// Start the Helm node.
    Run {
        /// Node name (overrides config file).
        #[arg(short, long)]
        name: Option<String>,

        /// Listen port (overrides config file, 0 = random).
        #[arg(short, long)]
        port: Option<u16>,

        /// Listen address.
        #[arg(long, default_value = "127.0.0.1")]
        listen: String,
    },

    /// Show node status and system information.
    Status,

    /// Manage the distributed store.
    Store {
        #[command(subcommand)]
        action: StoreCommands,
    },

    /// Run the GRG codec pipeline.
    Grg {
        #[command(subcommand)]
        action: GrgCommands,
    },

    /// Display version and module information.
    Info,

    /// Launch the interactive Moderator Bot.
    Moderator {
        /// Language code (en, ko, ja, zh, es, fr, de, pt, ar, hi, ru).
        #[arg(short = 'L', long)]
        lang: Option<String>,
    },

    /// Agent Womb — birth autonomous agents.
    Womb {
        #[command(subcommand)]
        action: WombCommands,
    },
}

#[derive(Subcommand, Debug)]
pub enum StoreCommands {
    /// Get a value by key.
    Get {
        /// Key to look up.
        key: String,
    },
    /// Put a key-value pair.
    Put {
        /// Key.
        key: String,
        /// Value.
        value: String,
    },
    /// List all keys.
    Keys,
    /// Show store statistics.
    Stats,
}

#[derive(Subcommand, Debug)]
pub enum GrgCommands {
    /// Encode data through the GRG pipeline.
    Encode {
        /// Input file or "-" for stdin.
        input: String,
        /// GRG mode: turbo, safety, rescue.
        #[arg(short, long, default_value = "turbo")]
        mode: String,
    },
    /// Decode GRG-encoded data.
    Decode {
        /// Input file.
        input: String,
    },
    /// Show codec statistics.
    Stats,
}

#[derive(Subcommand, Debug)]
pub enum WombCommands {
    /// Quick-birth a system agent with specified capability.
    Birth {
        /// Agent name.
        name: String,
        /// Primary capability (compute, storage, network, governance, security,
        /// codec, socratic, spawning, token, edge-api).
        #[arg(short, long, default_value = "compute")]
        capability: String,
    },
    /// Show womb status.
    Status,
    /// List available capabilities.
    Capabilities,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_parses_defaults() {
        let cli = Cli::parse_from(["helm"]);
        assert_eq!(cli.config, "helm.toml");
        assert_eq!(cli.log_level, "info");
        assert!(!cli.no_color);
        assert!(cli.command.is_none());
    }

    #[test]
    fn cli_parses_run() {
        let cli = Cli::parse_from(["helm", "run", "--name", "my-node", "--port", "9735"]);
        match cli.command {
            Some(Commands::Run { name, port, listen }) => {
                assert_eq!(name, Some("my-node".to_string()));
                assert_eq!(port, Some(9735));
                assert_eq!(listen, "127.0.0.1");
            }
            _ => panic!("expected Run command"),
        }
    }

    #[test]
    fn cli_parses_status() {
        let cli = Cli::parse_from(["helm", "status"]);
        assert!(matches!(cli.command, Some(Commands::Status)));
    }

    #[test]
    fn cli_parses_store_get() {
        let cli = Cli::parse_from(["helm", "store", "get", "my-key"]);
        match cli.command {
            Some(Commands::Store { action: StoreCommands::Get { key } }) => {
                assert_eq!(key, "my-key");
            }
            _ => panic!("expected Store Get command"),
        }
    }

    #[test]
    fn cli_parses_store_put() {
        let cli = Cli::parse_from(["helm", "store", "put", "key1", "value1"]);
        match cli.command {
            Some(Commands::Store { action: StoreCommands::Put { key, value } }) => {
                assert_eq!(key, "key1");
                assert_eq!(value, "value1");
            }
            _ => panic!("expected Store Put command"),
        }
    }

    #[test]
    fn cli_parses_grg_encode() {
        let cli = Cli::parse_from(["helm", "grg", "encode", "data.bin", "-m", "safety"]);
        match cli.command {
            Some(Commands::Grg { action: GrgCommands::Encode { input, mode } }) => {
                assert_eq!(input, "data.bin");
                assert_eq!(mode, "safety");
            }
            _ => panic!("expected Grg Encode command"),
        }
    }

    #[test]
    fn cli_parses_info() {
        let cli = Cli::parse_from(["helm", "info"]);
        assert!(matches!(cli.command, Some(Commands::Info)));
    }

    #[test]
    fn cli_parses_moderator() {
        let cli = Cli::parse_from(["helm", "moderator"]);
        assert!(matches!(cli.command, Some(Commands::Moderator { lang: None })));
    }

    #[test]
    fn cli_parses_moderator_with_lang() {
        let cli = Cli::parse_from(["helm", "moderator", "-L", "ko"]);
        match cli.command {
            Some(Commands::Moderator { lang }) => {
                assert_eq!(lang, Some("ko".to_string()));
            }
            _ => panic!("expected Moderator command"),
        }
    }

    #[test]
    fn cli_parses_womb_birth() {
        let cli = Cli::parse_from(["helm", "womb", "birth", "my-agent", "-c", "security"]);
        match cli.command {
            Some(Commands::Womb { action: WombCommands::Birth { name, capability } }) => {
                assert_eq!(name, "my-agent");
                assert_eq!(capability, "security");
            }
            _ => panic!("expected Womb Birth command"),
        }
    }

    #[test]
    fn cli_parses_womb_status() {
        let cli = Cli::parse_from(["helm", "womb", "status"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Womb { action: WombCommands::Status })
        ));
    }

    #[test]
    fn cli_parses_womb_capabilities() {
        let cli = Cli::parse_from(["helm", "womb", "capabilities"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Womb { action: WombCommands::Capabilities })
        ));
    }

    #[test]
    fn cli_verify_structure() {
        Cli::command().debug_assert();
    }
}
