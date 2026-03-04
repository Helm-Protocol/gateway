// src/p2p/mod.rs
pub mod kaleidoscope;
pub mod transport;
pub mod sovereign_router;

pub use kaleidoscope::{
    handle_incoming_stream, kaleidoscope_yamux_config,
    KaleidoscopePolicy, KaleidoscopeStats, SafeStream,
};
pub use transport::{build_secure_transport, get_bootstrap_peers};
pub use sovereign_router::{SovereignRouter, Sliver, SliverPriority};
