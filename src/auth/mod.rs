// src/auth/mod.rs
pub mod did_exchange;
pub mod types;

pub use did_exchange::{build_auth_message, DidExchangeService};
pub use types::{AgentContext, AuthError, GlobalPassport, LocalVisa, VisaIssuanceResponse};
