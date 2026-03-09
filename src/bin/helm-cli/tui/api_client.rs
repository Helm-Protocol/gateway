//! Async API Client for the Helm TUI.
//! Implements Ed25519 signing for all requests to ensure Sovereign Identity.

use anyhow::{Result, Context};
use reqwest::{Client, header};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use ed25519_dalek::{Signer, SigningKey};
use secrecy::{Secret, ExposeSecret};
#[derive(Debug, Clone)]
pub struct TuiApiClient {
    pub gateway_url: String,
    pub did: String,
    http: Client,
    // Note: The key should ideally be in a hardware enclave. 
    // Here we use secrecy for memory protection of the raw bytes.
    signing_key: Arc<Secret<[u8; 32]>>,
}

use std::sync::Arc;

impl TuiApiClient {
    pub fn new(gateway_url: String, did: String, key_bytes: [u8; 32]) -> Self {
        let mut headers = header::HeaderMap::new();
        headers.insert("X-Helm-Agent-ID", header::HeaderValue::from_str(&did).unwrap_or(header::HeaderValue::from_static("unknown")));

        let http = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(15))
            .build()
            .expect("Failed to create secure reqwest client");

        Self { 
            gateway_url, 
            did, 
            http, 
            signing_key: Arc::new(Secret::new(key_bytes))
        }
    }

    /// Generate a signature for the given payload and timestamp.
    /// Follows Kaleidoscope: All outbound data must be signed.
    fn sign_request(&self, payload: &str, timestamp: u64) -> String {
        let message = format!("{}.{}", timestamp, payload);
        let key_bytes = self.signing_key.expose_secret();
        let signing_key = SigningKey::from_bytes(key_bytes);
        let signature = signing_key.sign(message.as_bytes());
        hex::encode(signature.to_bytes())
    }

    fn get_timestamp_ms(&self) -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
    }

    /// Fetch balance with real-time signature verification.
    pub async fn fetch_balance(&self) -> Result<crate::tui::state::BalanceSnapshot> {
        let ts = self.get_timestamp_ms();
        let sig = self.sign_request("balance", ts);
        
        let url = format!("{}/v1/agent/me/balance", self.gateway_url);
        let resp = self.http.get(&url)
            .header("X-Helm-Timestamp", ts.to_string())
            .header("X-Helm-Signature", sig)
            .send()
            .await
            .context("Network failure during balance fetch")?
            .json()
            .await
            .context("Invalid balance response format")?;
            
        Ok(resp)
    }

    /// Fetch all active synthesis APIs.
    pub async fn fetch_synthesis_list(&self) -> Result<Vec<crate::tui::state::ApiListing>> {
        let url = format!("{}/v1/synthesis/list", self.gateway_url);
        let resp: serde_json::Value = self.http.get(&url)
            .send()
            .await
            .context("Failed to fetch synthesis list")?
            .json()
            .await
            .context("Invalid synthesis list format")?;
            
        let apis = resp["synthesis_apis"].as_array()
            .context("Missing synthesis_apis field")?;
            
        let mut listings = Vec::new();
        for a in apis {
            listings.push(crate::tui::state::ApiListing {
                api_id: a["api_id"].as_str().unwrap_or_default().to_string(),
                name: a["name"].as_str().unwrap_or_default().to_string(),
                description: a["description"].as_str().unwrap_or_default().to_string(),
                did_owner: a["did_owner"].as_str().unwrap_or_default().to_string(),
                price_v: a["price_v"].as_f64().unwrap_or(0.0),
                total_calls: a["total_calls"].as_u64().unwrap_or(0),
                components: a["components"].as_array()
                    .map(|arr| arr.iter().map(|v| v.as_str().unwrap_or_default().to_string()).collect())
                    .unwrap_or_default(),
                endpoint: a["endpoint_url"].as_str().unwrap_or_default().to_string(),
                creator_did: a["did_owner"].as_str().unwrap_or_default().to_string(),
                price: a["price_v"].as_f64().unwrap_or(0.0),
            });
        }
        Ok(listings)
    }

    /// Get the full API catalog for synthesis.
    pub async fn fetch_catalog(&self) -> Result<serde_json::Value> {
        let url = format!("{}/v1/synthesis/catalog", self.gateway_url);
        let resp = self.http.get(&url)
            .send()
            .await
            .context("Failed to fetch catalog")?
            .json()
            .await
            .context("Invalid catalog format")?;
        Ok(resp)
    }

    /// Create a new Synthesis API product.
    pub async fn create_synthesis(
        &self, 
        name: String, 
        description: String, 
        components: Vec<serde_json::Value>, 
        price_micro: u64
    ) -> Result<serde_json::Value> {
        let ts = self.get_timestamp_ms();
        let payload = serde_json::json!({
            "name": name,
            "description": description,
            "components": components,
            "price_micro": price_micro,
            "timestamp": ts
        });
        let payload_str = payload.to_string();
        let sig = self.sign_request(&payload_str, ts);
        
        let url = format!("{}/v1/synthesis/create", self.gateway_url);
        let resp = self.http.post(&url)
            .header("X-Helm-Timestamp", ts.to_string())
            .header("X-Helm-Signature", sig)
            .json(&payload)
            .send()
            .await
            .context("Synthesis creation request failed")?
            .json()
            .await
            .context("Failed to parse synthesis creation response")?;
            
        Ok(resp)
    }

    /// Register as a broker for a Synthesis API to earn 20% commission.
    pub async fn broker_synthesis(
        &self, 
        creator_did: &str, 
        api_id: &str
    ) -> Result<serde_json::Value> {
        let ts = self.get_timestamp_ms();
        let payload = serde_json::json!({
            "broker_did": self.did,
            "timestamp": ts
        });
        let payload_str = payload.to_string();
        let sig = self.sign_request(&payload_str, ts);
        
        let url = format!("{}/v1/synth/{}/{}/broker", self.gateway_url, creator_did, api_id);
        let resp = self.http.post(&url)
            .header("X-Helm-Timestamp", ts.to_string())
            .header("X-Helm-Signature", sig)
            .json(&payload)
            .send()
            .await
            .context("Brokering request failed")?
            .json()
            .await
            .context("Failed to parse brokering response")?;
            
        Ok(resp)
    }

    /// Call a Synthesis API.
    pub async fn call_synthesis(
        &self, 
        creator_did: &str, 
        api_id: &str, 
        input: serde_json::Value
    ) -> Result<serde_json::Value> {
        let ts = self.get_timestamp_ms();
        let payload = serde_json::json!({
            "input": input,
            "timestamp": ts
        });
        let payload_str = payload.to_string();
        let sig = self.sign_request(&payload_str, ts);
        
        let url = format!("{}/v1/synth/{}/{}", self.gateway_url, creator_did, api_id);
        let resp = self.http.post(&url)
            .header("X-Helm-Timestamp", ts.to_string())
            .header("X-Helm-Signature", sig)
            .json(&payload)
            .send()
            .await
            .context("Synthesis call failed")?
            .json()
            .await
            .context("Failed to parse synthesis call result")?;
            
        Ok(resp)
    }
}
