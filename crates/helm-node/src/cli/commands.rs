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
    fn cli_verify_structure() {
        Cli::command().debug_assert();
    }
}
