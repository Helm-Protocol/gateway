// src/p2p/mod.rs
pub mod kaleidoscope;
pub mod transport;

pub use kaleidoscope::{
    handle_incoming_stream, kaleidoscope_yamux_config,
    KaleidoscopePolicy, KaleidoscopeStats, SafeStream,
};
pub use transport::{build_secure_transport, get_bootstrap_peers};
