// src/metrics.rs
// ─────────────────────────────────────────────────────────────────────────────
// 공유 원자 카운터 — 게이트웨이 모든 모듈이 여기에 쓰고, 대시보드가 읽는다
// ─────────────────────────────────────────────────────────────────────────────

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;

pub struct GatewayMetrics {
    // ─ 트래픽 ─
    pub tps:           AtomicU64,
    pub total_calls:   AtomicU64,
    pub tps_a:         AtomicU64,
    pub tps_b:         AtomicU64,
    pub tps_c:         AtomicU64,
    pub tps_d:         AtomicU64,

    // ─ 수익 (BNKR × 10^6 고정소수점) ─
    pub total_bnkr_micro:   AtomicU64, // 1 BNKR = 1_000_000 units
    pub treasury_usd_cents: AtomicU64, // $1 = 100 cents

    // ─ 보안 ─
    pub blocked_attacks: AtomicU64,
    pub nonce_replays:   AtomicU64,
    pub active_visas:    AtomicU64,

    // ─ 캐시 ─
    pub cache_hits:   AtomicU64,
    pub cache_misses: AtomicU64,

    // ─ 결제 ─
    pub rollup_queue: AtomicU64,

    // ─ G-Metric 분포 (10구간) ─
    pub g_buckets: [AtomicU64; 10],
}

impl GatewayMetrics {
    pub fn new() -> Self {
        Self {
            tps:           AtomicU64::new(0),
            total_calls:   AtomicU64::new(0),
            tps_a:         AtomicU64::new(0),
            tps_b:         AtomicU64::new(0),
            tps_c:         AtomicU64::new(0),
            tps_d:         AtomicU64::new(0),
            total_bnkr_micro:   AtomicU64::new(0),
            treasury_usd_cents: AtomicU64::new(0),
            blocked_attacks: AtomicU64::new(0),
            nonce_replays:   AtomicU64::new(0),
            active_visas:    AtomicU64::new(0),
            cache_hits:   AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            rollup_queue: AtomicU64::new(0),
            g_buckets: Default::default(),
        }
    }

    // ─ 헬퍼 ─
    pub fn total_bnkr_f64(&self) -> f64 {
        self.total_bnkr_micro.load(Relaxed) as f64 / 1_000_000.0
    }

    pub fn treasury_usd_f64(&self) -> f64 {
        self.treasury_usd_cents.load(Relaxed) as f64 / 100.0
    }

    pub fn cache_hit_rate(&self) -> u8 {
        let hits   = self.cache_hits.load(Relaxed);
        let misses = self.cache_misses.load(Relaxed);
        let total  = hits + misses;
        if total == 0 { return 0; }
        ((hits as f64 / total as f64) * 100.0) as u8
    }

    pub fn g_distribution_snapshot(&self) -> [u32; 10] {
        let mut arr = [0u32; 10];
        for (i, b) in self.g_buckets.iter().enumerate() {
            arr[i] = b.load(Relaxed) as u32;
        }
        arr
    }

    // ─ 편의 기록 메서드 ─
    pub fn record_bnkr(&self, bnkr: f64) {
        let micro = (bnkr * 1_000_000.0) as u64;
        self.total_bnkr_micro.fetch_add(micro, Relaxed);
        let cents = (bnkr * 50.0) as u64; // 1 BNKR ≈ $0.50
        self.treasury_usd_cents.fetch_add(cents, Relaxed);
    }

    pub fn record_g_score(&self, g: f32) {
        let bucket = (g * 10.0).min(9.0) as usize;
        self.g_buckets[bucket].fetch_add(1, Relaxed);
    }

    pub fn record_cache_hit(&self)  { self.cache_hits.fetch_add(1, Relaxed);   }
    pub fn record_cache_miss(&self) { self.cache_misses.fetch_add(1, Relaxed); }
    pub fn record_blocked(&self)    { self.blocked_attacks.fetch_add(1, Relaxed); }
    pub fn record_replay(&self)     { self.nonce_replays.fetch_add(1, Relaxed); }
    pub fn add_rollup_ticket(&self) { self.rollup_queue.fetch_add(1, Relaxed); }
    pub fn inc_call(&self)          { self.total_calls.fetch_add(1, Relaxed); }
}

impl Default for GatewayMetrics {
    fn default() -> Self { Self::new() }
}
