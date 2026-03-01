//! Helm Sense API Gateway — HTTP layer over the Helm Protocol.
//!
//! This module implements the actual HTTP server that was missing
//! from the codebase (gateway_commands.rs had the CLI structure
//! but zero server implementation).
//!
//! ## Architecture
//!
//! ```
//! helm gateway start --port 8080
//!         │
//!         ▼
//! [Axum HTTP Server]
//!         │
//!   ┌─────┴──────────────────────────┐
//!   │     Auth Middleware            │
//!   │  (DID Bearer token check)      │
//!   └─────┬──────────────────────────┘
//!         │
//!   ┌─────▼──────────────────────────────────────────────────────┐
//!   │                    Route Handlers                          │
//!   │  F-Line: Sense Cortex  → helm-engine QKV-G + Socratic Claw │
//!   │  G-Line: Sync-O        → helm-engine GRG pipeline          │
//!   │  E-Line: Sense Memory  → helm-store TieredCache/LWW CRDT   │
//!   │  D-Line: Helm Score    → helm-identity ReputationScore      │
//!   │  Pool:   HelmPool      → VIRTUAL ledger + StakePool         │
//!   │  Graph:  Earnings      → billing.rs referral tracking       │
//!   └────────────────────────────────────────────────────────────┘
//! ```

pub mod auth;
pub mod db;
pub mod handlers;
pub mod pricing;
pub mod server;
pub mod state;
pub mod x402;

#[cfg(test)]
mod tests;

use anyhow::Result;
use std::net::SocketAddr;
use tokio::net::TcpListener;

use self::server::build_router;
use self::state::AppState;

/// Start the Helm Sense API Gateway HTTP server.
pub async fn start_gateway(port: u16, public_url: Option<String>) -> Result<()> {
    let state = AppState::new();
    let router = build_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;

    let public_url = public_url.unwrap_or_else(|| format!("http://localhost:{}", port));

    println!();
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║          ⚓  Helm Sense API Gateway                      ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  Listening on:  http://0.0.0.0:{:<30}║", port);
    println!("║  Public URL:    {:<42}║", public_url);
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  Lines:                                                  ║");
    println!("║    F-Line  Sense Cortex    POST /v1/sense/cortex         ║");
    println!("║    G-Line  Sync-O (GRG)   POST /v1/synco/stream         ║");
    println!("║    E-Line  Sense Memory   GET/PUT /v1/sense/memory/:key  ║");
    println!("║    D-Line  Helm Score     GET /v1/agent/:did/helm-score   ║");
    println!("║    Pool    HelmPool       POST /v1/pool                  ║");
    println!("║    Market  Marketplace    POST /v1/marketplace/post      ║");
    println!("║    Graph   Earnings       GET /v1/agent/:did/earnings    ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  Packages:                                               ║");
    println!("║    Alpha Hunt       POST /v1/package/alpha-hunt  10 VIRT ║");
    println!("║    Protocol Shield  POST /v1/package/protocol-shield     ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  Start:  POST {}/v1/agent/boot              ║", " ".repeat(27 - public_url.len().min(27)));
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();
    println!("  Every agent is a node. Every node is sovereign.");
    println!();

    tracing::info!("Helm Sense API Gateway started on port {}", port);

    axum::serve(listener, router).await?;
    Ok(())
}
