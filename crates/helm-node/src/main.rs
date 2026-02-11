use anyhow::Result;
use std::path::Path;
use tracing_subscriber::EnvFilter;

use helm_core::{HelmConfig, Runtime};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("helm=info".parse()?),
        )
        .init();

    let config = HelmConfig::load_or_default(Path::new("helm.toml"));

    tracing::info!("Helm Protocol v{}", env!("CARGO_PKG_VERSION"));
    tracing::info!("Every agent is a node. Every node is sovereign.");

    let runtime = Runtime::new(config);
    runtime.run().await
}
