//! Socratic Claw — Gap-Aware Decision Process.
//!
//! The Socratic Claw intercepts agent execution at every tick.
//! When the QKV-G attention engine detects a knowledge gap (G > threshold),
//! execution is halted and the agent enters Socratic questioning mode.
//!
//! # Flow
//!
//! 1. Agent tick begins
//! 2. Socratic Claw evaluates current G-metric from QKV-G engine
//! 3. If G < threshold → proceed (agent executes normally)
//! 4. If G >= threshold → halt + enter questioning loop
//! 5. Gap vector compressed via MLA Down-Projection → stored in GapRepository
//! 6. Question generated from Up-Projection of gap latent
//! 7. When answer received → gap re-evaluated → if resolved, resume execution
//!
//! # Self-Training Loop
//!
//! Answers absorbed into the QKV-G key-value cache:
//! - K ← question context vector
//! - V ← answer vector
//! - G re-evaluated: if G_new < threshold → gap filled → resume

pub mod claw;
pub mod gap_repo;
