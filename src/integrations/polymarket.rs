use serde::Deserialize;
use reqwest::Client;
use std::time::Duration;

const GAMMA_API: &str = "https://gamma-api.polymarket.com";

#[derive(Debug, Deserialize, Clone)]
pub struct Market {
    pub id: String,
    pub question: String,
    pub description: String,
    pub active: bool,
}

pub struct PolymarketCrawler {
    client: Client,
}

impl Default for PolymarketCrawler {
    fn default() -> Self {
        Self {
            client: Client::builder().timeout(Duration::from_secs(10)).build().unwrap(),
        }
    }
}

impl PolymarketCrawler {
    pub async fn fetch_active_markets(&self, limit: usize) -> Result<Vec<Market>, Box<dyn std::error::Error>> {
        let url = format!("{}/markets?limit={}&active=true", GAMMA_API, limit);
        let resp = self.client.get(&url).send().await?;
        let markets: Vec<Market> = resp.json().await?;
        Ok(markets)
    }

    /// Convert text to an embedding (semantic if fastembed is on, else fallback)
    pub fn embed_text(text: &str) -> Vec<f32> {
        let raw_vec = {
            #[cfg(feature = "fastembed")]
            {
                use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};
                let model = TextEmbedding::try_new(
                    InitOptions::new(EmbeddingModel::BGESmallENV15)
                        .with_show_download_progress(false)
                );
                if let Ok(mut m) = model {
                    if let Ok(e) = m.embed(vec![text], None) {
                        if let Some(v) = e.into_iter().next() {
                            v
                        } else { vec![0.0; 8] }
                    } else { vec![0.0; 8] }
                } else { vec![0.0; 8] }
            }
            #[cfg(not(feature = "fastembed"))]
            {
                // Fallback: 8D hash-based embedding
                let mut vector = vec![0.0f32; 8];
                let bytes = text.as_bytes();
                if bytes.len() >= 3 {
                    for i in 0..bytes.len() - 2 {
                        let n_gram = &bytes[i..i+3];
                        let mut hash = 0u32;
                        for &b in n_gram { hash = hash.wrapping_add(b as u32).wrapping_mul(31); }
                        let bucket = (hash % 8) as usize;
                        vector[bucket] += 1.0;
                    }
                }
                vector
            }
        };

        Self::project_to_8d(&raw_vec)
    }

    /// Project any dimension vector to 8D space
    pub fn project_to_8d(v: &[f32]) -> Vec<f32> {
        if v.len() == 8 {
            return crate::filter::g_metric::normalize(v);
        }
        let mut projected = vec![0.0f32; 8];
        let chunk_size = v.len() / 8;
        for i in 0..8 {
            let start = i * chunk_size;
            let end = if i == 7 { v.len() } else { (i + 1) * chunk_size };
            let sum: f32 = v[start..end].iter().sum();
            projected[i] = sum / (end - start) as f32;
        }
        crate::filter::g_metric::normalize(&projected)
    }
}
