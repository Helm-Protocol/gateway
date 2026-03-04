# Gemini CLI & Gateway Production Engineering Guide

This project adheres to the **"Rust at Scale"** security philosophy pioneered by Meta (WhatsApp). We transition from legacy safety patterns to a proactive, memory-safe, and high-performance architecture using **Rust and Axum**.

## 1. Core Engineering Philosophies

*   **Defense in Depth & Memory Safety:** Following WhatsApp's migration for media processing, we eliminate memory corruption risks. Use `rustls` instead of OpenSSL. **Strictly prohibit `panic!`, `unwrap()`, and `expect()`.**
*   **Secret Zeroization:** Protect API keys in memory using the `secrecy` crate. Prevent leakage in memory dumps, logs, or crash reports.
*   **Kaleidoscope Pattern (Input/Output Validation):** Treat all external data (Gemini API responses, CLI inputs) as untrusted. Perform rigorous type validation during deserialization.
*   **Observability over Debugging:** Replace `println!` with structured, asynchronous-aware logging via the `tracing` ecosystem.

## 2. Production-Grade Stack (Cargo.toml Standard)

```toml
[dependencies]
# Runtime & Web Framework
tokio = { version = "1.0", features = ["full"] }
axum = { version = "0.7", features = ["macros"] }

# Network & Security
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
secrecy = "0.8"
rustls = "0.23"

# Serialization & CLI
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
clap = { version = "4.5", features = ["derive"] }

# Error & Observability
thiserror = "1.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
```

## 3. Implementation Standards

### 3.1 Error Architecture (`src/error.rs`)
Never settle for string-based errors. Define a comprehensive, typed error system.
```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Network failure: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Security: Missing or unsafe API Key")]
    AuthError,
    #[error("Validation: Invalid response structure (Kaleidoscope failure)")]
    ValidationError,
}
```

### 3.2 Secure Client Pattern (`src/client.rs`)
Implement explicit timeouts and memory-safe secret handling.
```rust
pub struct SecureClient {
    http: reqwest::Client,
    key: secrecy::Secret<String>,
}

impl SecureClient {
    pub async fn call_gemini(&self, prompt: &str) -> Result<String, AppError> {
        // Implementation must use .error_for_status() and safe deserialization
    }
}
```

## 4. Release & Optimization (Meta Standard)

### 4.1 Binary Optimization
Minimize runtime footprint and attack surface.
```toml
[profile.release]
opt-level = "z"     # Optimize for size
lto = true          # Link Time Optimization
codegen-units = 1   # Maximize optimization quality
panic = "abort"     # Immediate termination on panic
strip = true        # Remove debug symbols
```

### 4.2 CI/CD Enforcement
- **Clippy:** `cargo clippy -- -D warnings` is mandatory.
- **Fuzzing:** Unit tests must cover edge cases (null bytes, excessive length, malformed JSON).
- **Audit:** Periodic `cargo audit` to check for crate vulnerabilities.
