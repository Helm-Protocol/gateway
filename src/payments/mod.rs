// src/payments/mod.rs
pub mod x402;
pub use x402::{
    PaymentError, PaymentQuote, PaymentTicket, PaymentVerdict,
    ProcessedPayment, SettlementBatch, X402PaymentProcessor,
};
