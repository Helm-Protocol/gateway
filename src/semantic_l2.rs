// gateway/src/semantic_l2.rs
// ═══════════════════════════════════════════════════════════════
// AGENT SOVEREIGN PROTOCOL — L2 SEMANTIC CORE (UNIVERSAL 8D)
// ═══════════════════════════════════════════════════════════════

use serde::{Serialize, Deserialize};
use redis::AsyncCommands;
use redis::aio::ConnectionManager;

/// Universal 8D G-Metric Dimensions (Axiom Based)
pub const DIM_NAMES: [&str; 8] = [
    "conservation",     // d0: thermodynamics 1st law
    "identity",         // d1: set theory extensionality
    "integrity",        // d2: logical consistency
    "proportionality",  // d3: linear algebra scaling
    "transitivity",     // d4: order theory
    "boundary",         // d5: topology open/closed sets
    "evolution",        // d6: dynamical systems
    "symmetry",         // d7: group theory
];

const DIM_KEYWORDS: [[&str; 14]; 8] = [
    // d0: conservation
    ["resource", "balance", "spend", "cost", "conserve", "preserve", "maintain",
     "account", "budget", "supply", "energy", "reserve", "sustain", "economy"],
    // d1: identity
    ["identity", "unique", "self", "did", "who", "individual", "person", "agent",
     "name", "signature", "auth", "distinguish", "singular", "irreducible"],
    // d2: integrity
    ["honest", "truth", "integrity", "declare", "admit", "insufficient", "unknown",
     "gap", "limit", "verify", "audit", "consistent", "transparent", "accurate"],
    // d3: proportionality
    ["fair", "price", "value", "proportion", "exchange", "trade", "worth", "equal",
     "reward", "merit", "correlate", "measure", "weight", "scale"],
    // d4: transitivity
    ["trust", "referral", "propagate", "network", "graph", "chain", "flow", "relay",
     "transitive", "connect", "bridge", "path", "route", "relationship"],
    // d5: boundary
    ["boundary", "sovereign", "private", "protect", "secure", "inject", "isolate",
     "separate", "domain", "membrane", "filter", "shield", "wall", "permission"],
    // d6: evolution
    ["evolve", "grow", "learn", "progress", "advance", "develop", "create", "new",
     "novel", "improve", "iterate", "adapt", "transform", "emerge"],
    // d7: symmetry
    ["balance", "symmetry", "fair", "equal", "harmony", "restore", "equilibrium",
     "mutual", "reciprocal", "stable", "converge", "universal", "invariant", "beauty"],
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticNode {
    pub id: String,
    pub vector: [f32; 8],
    pub g_vector: [f32; 8],  
    pub g_score: f32,         
    pub metadata: String,
}

pub struct SemanticL2 {
    pub redis_manager: ConnectionManager,
}

impl SemanticL2 {
    pub async fn new(redis_url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let manager = ConnectionManager::new(client).await?;
        Ok(Self { redis_manager: manager })
    }

    /// Store a node in Redis with Universal 8D metadata
    pub async fn add_node(&self, node: &SemanticNode) -> Result<(), Box<dyn std::error::Error>> {
        let mut con = self.redis_manager.clone();
        let json_data = serde_json::to_string(node)?;
        
        // Unify to Hash storage for consistency and performance
        redis::pipe()
            .atomic()
            .hset("semantic:nodes", &node.id, json_data)
            .expire("semantic:nodes", 604800) // 1 week TTL
            .query_async::<_, ()>(&mut con)
            .await?;
            
        Ok(())
    }

    pub async fn get_all_nodes(&self) -> Result<Vec<SemanticNode>, redis::RedisError> {
        let mut con = self.redis_manager.clone();
        // Read all nodes from the unified Hash
        let nodes_map: std::collections::HashMap<String, String> = con.hgetall("semantic:nodes").await?;
        let mut nodes = Vec::new();
        for json_str in nodes_map.values() {
            if let Ok(node) = serde_json::from_str(json_str) {
                nodes.push(node);
            }
        }
        Ok(nodes)
    }

    /// compute_8d_vector: TLA+ Axiom based weight balancing
    pub fn compute_8d_vector(text: &str) -> [f32; 8] {
        let mut v = [0.5f32; 8]; // Base neutral state
        let text_lower = text.to_lowercase();

        for i in 0..8 {
            let mut hits = 0;
            for kw in DIM_KEYWORDS[i] {
                if text_lower.contains(kw) { hits += 1; }
            }
            // Keyword boost: 0.20 per hit (max 0.45)
            let boost = (hits as f32 * 0.20).min(0.45);
            v[i] = (v[i] + boost).clamp(0.0, 1.0);

            // Semantic hash offset: +/- 0.1 (Precision Salt)
            let hash = xxhash_rust::xxh3::xxh3_64(text.as_bytes());
            let offset = ((hash % 200) as f32 / 1000.0) - 0.1;
            v[i] = (v[i] + offset).clamp(0.0, 1.0);
        }
        v
    }

    /// compute_g_vector: Nearest-node dimensional gap analysis
    pub fn compute_g_vector(query: &[f32; 8], nodes: &[SemanticNode]) -> [f32; 8] {
        let nearest = nodes.iter()
            .min_by(|a, b| {
                let da = Self::euclidean_dist(query, &a.vector);
                let db = Self::euclidean_dist(query, &b.vector);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            });

        let Some(nearest) = nearest else { return [1.0; 8] };

        let mut g_vec = [0.0f32; 8];
        for d in 0..8 {
            let gap = (query[d] - nearest.vector[d]).abs();
            g_vec[d] = (gap * 5.0).tanh(); // Amplify small diffs
        }
        g_vec
    }

    /// scalar_from_g_vector: Weighted RMS based on Axiom priority
    pub fn scalar_from_g_vector(g_vec: [f32; 8]) -> f32 {
        // d2 (Integrity) and d7 (Symmetry) are most critical
        let weights: [f32; 8] = [1.0, 1.0, 1.5, 1.0, 1.0, 1.2, 1.3, 1.3];
        let mut weighted_sum = 0.0f32;
        let mut total_weight = 0.0f32;
        for i in 0..8 {
            weighted_sum += weights[i] * g_vec[i].powi(2);
            total_weight += weights[i];
        }
        (weighted_sum / total_weight).sqrt()
    }

    pub async fn calculate_g_vector(&self, q: [f32; 8]) -> (f32, [f32; 8]) {
        let nodes = self.get_all_nodes().await.unwrap_or_default();
        if nodes.is_empty() { return (1.0, [1.0; 8]); }

        let g_vec = Self::compute_g_vector(&q, &nodes);
        let g_score = Self::scalar_from_g_vector(g_vec);

        if g_score > 0.85 {
            let dim_names = [
                "conservation", "identity", "integrity", "proportionality",
                "transitivity", "boundary", "evolution", "symmetry"
            ];
            let mut missing = Vec::new();
            for (i, &g) in g_vec.iter().enumerate() {
                if g > 0.6 { missing.push(dim_names[i]); }
            }
            
            tokio::spawn(async move {
                Self::trigger_telegram_sos(g_score, "Semantic-8D", &format!("Critical Gap! Missing: {:?}", missing)).await;
            });
        }

        (g_score, g_vec)
    }

    pub async fn trigger_telegram_sos(g_score: f32, domain: &str, context: &str) {
        let bot_token = std::env::var("TELEGRAM_TOKEN").unwrap_or_default();
        let chat_id = std::env::var("ALLOWED_CHAT_ID").unwrap_or_default();
        if bot_token.is_empty() || chat_id.is_empty() { return; }
        let message = format!("🚨 [Semantic SOS]\nG-Score: {:.2}\nDomain: {}\nContext: {}\n\nThe Axioms are calling!", g_score, domain, context);
        let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
        let _ = reqwest::Client::new().post(&url).json(&serde_json::json!({"chat_id": chat_id, "text": message})).send().await;
    }

    pub fn euclidean_dist(a: &[f32; 8], b: &[f32; 8]) -> f32 {
        a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum::<f32>().sqrt()
    }

    pub fn quantize_8d(v: [f32; 8]) -> [f32; 8] {
        let f_v = Self::closest_d8(v);
        let mut v_shifted = [0.0f32; 8];
        for i in 0..8 { v_shifted[i] = v[i] - 0.5; }
        let g_v = Self::closest_d8(v_shifted);
        let mut g_v_final = [0.0f32; 8];
        for i in 0..8 { g_v_final[i] = g_v[i] + 0.5; }
        if Self::euclidean_dist(&v, &f_v) < Self::euclidean_dist(&v, &g_v_final) { f_v } else { g_v_final }
    }

    fn closest_d8(v: [f32; 8]) -> [f32; 8] {
        let mut rounded = [0.0f32; 8];
        let mut sum = 0;
        let mut diffs = [0.0f32; 8];
        for i in 0..8 {
            rounded[i] = v[i].round();
            sum += rounded[i] as i32;
            diffs[i] = v[i] - rounded[i];
        }
        if sum % 2 != 0 {
            let mut best_idx = 0;
            let mut max_diff = -1.0;
            for i in 0..8 {
                let d = diffs[i].abs();
                if d > max_diff { max_diff = d; best_idx = i; }
            }
            if diffs[best_idx] > 0.0 { rounded[best_idx] += 1.0; } else { rounded[best_idx] -= 1.0; }
        }
        rounded
    }
}
