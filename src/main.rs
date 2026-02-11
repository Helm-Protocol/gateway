use anyhow::Result;
use tracing_subscriber::EnvFilter;

mod node;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("helm=info".parse()?))
        .init();

    tracing::info!("Helm Protocol v{}", env!("CARGO_PKG_VERSION"));
    tracing::info!("Every agent is a node. Every node is sovereign.");

    node::run().await
}
