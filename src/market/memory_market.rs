// gateway/src/market/memory_market.rs
// ═══════════════════════════════════════════════════════════════
// HELM MEMORY MARKET (Phase 3) — Autonomous Knowledge Exchange
// ═══════════════════════════════════════════════════════════════

use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use uuid::Uuid;
use crate::billing::CREATOR_SHARE_BP;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryListing {
    pub id: Uuid,
    pub creator_did: String,
    pub knowledge_hash: String,
    pub lockin_score: f32,
    pub depth: u32,
    pub price_bnkr: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PurchaseResult {
    pub listing_id: Uuid,
    pub creator_share_bnkr: f64,
    pub helm_share_bnkr: f64,
    pub success: bool,
}

pub struct HelmMemoryMarket {
    listings: RwLock<HashMap<Uuid, MemoryListing>>,
    base_price: f64,
}

impl HelmMemoryMarket {
    pub fn new(base_price: f64) -> Self {
        Self {
            listings: RwLock::new(HashMap::new()),
            base_price,
        }
    }

    /// List agent knowledge in the market
    pub fn list_knowledge(
        &self,
        creator_did: &str,
        k_hash: &str,
        lockin: f32,
        depth: u32,
    ) -> Result<Uuid, &'static str> {
        if lockin < 0.60 {
            return Err("Lock-in score too low for market entry (min 0.60)");
        }

        // Price Formula: BASE * lockin * sqrt(depth)
        let price = self.base_price * (lockin as f64) * (depth as f64).sqrt();
        let id = Uuid::new_v4();

        let listing = MemoryListing {
            id,
            creator_did: creator_did.to_string(),
            knowledge_hash: k_hash.to_string(),
            lockin_score: lockin,
            depth,
            price_bnkr: price,
        };

        self.listings.write().insert(id, listing);
        Ok(id)
    }

    /// Purchase knowledge with 80/20 distribution
    pub fn purchase(&self, id: Uuid) -> Result<PurchaseResult, &'static str> {
        let listings = self.listings.read();
        let listing = listings.get(&id).ok_or("Listing not found")?;

        let total_price = listing.price_bnkr;
        let creator_share = total_price * (CREATOR_SHARE_BP as f64 / 10_000.0);
        let helm_share = total_price - creator_share;

        Ok(PurchaseResult {
            listing_id: id,
            creator_share_bnkr: creator_share,
            helm_share_bnkr: helm_share,
            success: true,
        })
    }

    pub fn get_listings(&self) -> Vec<MemoryListing> {
        self.listings.read().values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_listing_and_price() {
        let market = HelmMemoryMarket::new(100.0);
        let id = market.list_knowledge("did:helm:a1", "hash1", 0.8, 16).unwrap();
        
        let listings = market.get_listings();
        assert_eq!(listings.len(), 1);
        // 100 * 0.8 * sqrt(16) = 100 * 0.8 * 4 = 320
        assert_eq!(listings[0].price_bnkr, 320.0);
    }

    #[test]
    fn test_lockin_guard() {
        let market = HelmMemoryMarket::new(100.0);
        let result = market.list_knowledge("did:helm:a1", "hash1", 0.5, 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_fee_split_80_20() {
        let market = HelmMemoryMarket::new(100.0);
        let id = market.list_knowledge("did:helm:a1", "hash1", 1.0, 1).unwrap();
        let purchase = market.purchase(id).unwrap();
        
        assert_eq!(purchase.creator_share_bnkr, 80.0);
        assert_eq!(purchase.helm_share_bnkr, 20.0);
    }
}
