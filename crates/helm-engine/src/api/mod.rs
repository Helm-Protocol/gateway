//! Helm Engine API layer.
//!
//! Two API surfaces:
//! - **Edge API**: Public-facing, metered API for external agents.
//!   Agents using other protocols (bnkr etc.) pay tokens/coins to access.
//!   15% of revenue flows back to the Helm treasury.
//!
//! - **Core (Hidden) API**: Internal API for the distributed core brain.
//!   Sends alerts, requests, and questions to individual agents.
//!   Powers the autonomous agent and self-security system.

pub mod edge;
pub mod core_api;
pub mod billing;
