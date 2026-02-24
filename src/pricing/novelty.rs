// src/pricing/novelty.rs
// [Day 3] Two-Part Tariff 가격 엔진
//
// 구조:
//   Base Toll:      0.0001 BNKR (항상, 스팸방지)
//   Novelty Premium: G에 비례 (0.01~0.08 BNKR)
//
// 철학 (Jeff Dean):
//   "물을 뜨러 오는 건 공짜(Base Toll 최소화),
//    금가루(Delta)가 나올 때만 크게 떼라"

use serde::{Deserialize, Serialize};

/// 가격 계산 결과
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceQuote {
    /// 기본 통행료 (항상 부과)
    pub base_toll_bnkr: f64,
    /// 신규 정보 프리미엄 (G 기반)
    pub novelty_premium_bnkr: f64,
    /// 총액
    pub total_bnkr: f64,
    /// G-Metric 점수
    pub g_score: f32,
    /// 가격 등급
    pub tier: PriceTier,
    /// USD 환산 (BNKR = $0.50 가정)
    pub total_usd: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PriceTier {
    /// Free Tier (100 calls 이내)
    Free,
    /// Base Toll만 (중복/스팸 드롭)
    BaseTollOnly,
    /// 정상 과금 (신규 정보)
    Standard,
    /// 프리미엄 (신규 토픽, G 높음)
    Premium,
}

/// Two-Part Tariff 가격 계산기
#[derive(Debug, Clone)]
pub struct TariffEngine {
    /// Base Toll (BNKR)
    pub base_toll: f64,
    /// Novelty 계수 (G당 단가)
    pub novelty_coefficient: f64,
    /// BNKR/USD 환율
    pub bnkr_usd_rate: f64,
}

impl Default for TariffEngine {
    fn default() -> Self {
        Self {
            base_toll: 0.0001,         // 0.0001 BNKR = $0.00005
            novelty_coefficient: 0.10,  // G 0.1 증가 → 0.01 BNKR 추가
            bnkr_usd_rate: 0.50,       // 1 BNKR = $0.50
        }
    }
}

impl TariffEngine {
    /// G-Metric 기반 가격 계산
    ///
    /// G 범위별 price:
    ///   G < 0.10: Base Toll만 (중복)
    ///   G 0.10~0.80: Base + (G - 0.10) × 0.10
    ///   G > 0.80: Base Toll만 (스팸)
    ///   G = 1.0 (신규 토픽): 0.05 BNKR 고정
    pub fn calculate(
        &self,
        g_score: f32,
        is_free_tier: bool,
        is_new_topic: bool,
    ) -> PriceQuote {
        // Free Tier: 100 calls까지 무료
        if is_free_tier {
            return PriceQuote {
                base_toll_bnkr: 0.0,
                novelty_premium_bnkr: 0.0,
                total_bnkr: 0.0,
                g_score,
                tier: PriceTier::Free,
                total_usd: 0.0,
            };
        }

        // 신규 토픽 프리미엄
        if is_new_topic {
            let total = 0.05;
            return PriceQuote {
                base_toll_bnkr: self.base_toll,
                novelty_premium_bnkr: total - self.base_toll,
                total_bnkr: total,
                g_score,
                tier: PriceTier::Premium,
                total_usd: total * self.bnkr_usd_rate,
            };
        }

        const G_MIN: f32 = 0.10;
        const G_MAX: f32 = 0.80;

        let (novelty, tier) = if g_score < G_MIN || g_score > G_MAX {
            // 중복 또는 스팸 — Base Toll만
            (0.0, PriceTier::BaseTollOnly)
        } else {
            // 골디락스 존 — Novelty Premium
            let premium = (g_score - G_MIN) as f64 * self.novelty_coefficient;
            let premium = (premium * 100_000.0).round() / 100_000.0; // 소수점 5자리
            let tier = if g_score > 0.6 { PriceTier::Premium } else { PriceTier::Standard };
            (premium, tier)
        };

        let total = self.base_toll + novelty;

        PriceQuote {
            base_toll_bnkr: self.base_toll,
            novelty_premium_bnkr: novelty,
            total_bnkr: total,
            g_score,
            tier,
            total_usd: total * self.bnkr_usd_rate,
        }
    }

    /// 일 수익 추정 (시뮬레이션)
    pub fn simulate_daily_revenue(
        &self,
        total_agents: u64,
        calls_per_agent_per_day: u64,
    ) -> RevenueSimulation {
        let total_calls = total_agents * calls_per_agent_per_day;

        // 분포 (Jeff Dean 보고서 기반)
        let exact_dup_rate = 0.20;    // Layer2 중복
        let semantic_dup_rate = 0.20; // Layer2 의미 중복
        let spam_rate = 0.20;         // Layer3 스팸
        let novel_rate = 0.30;        // 골디락스
        let new_topic_rate = 0.10;    // 신규 토픽

        let base_toll_all = total_calls as f64 * self.base_toll;

        let novel_calls = (total_calls as f64 * novel_rate) as u64;
        let novel_avg_g = 0.45_f64;
        let novel_premium = novel_calls as f64 * (novel_avg_g - 0.10) * self.novelty_coefficient;

        let new_topic_calls = (total_calls as f64 * new_topic_rate) as u64;
        let new_topic_revenue = new_topic_calls as f64 * 0.05;

        let total_bnkr = base_toll_all + novel_premium + new_topic_revenue;

        RevenueSimulation {
            total_calls,
            total_agents,
            base_toll_revenue_bnkr: base_toll_all,
            novelty_premium_revenue_bnkr: novel_premium,
            new_topic_revenue_bnkr: new_topic_revenue,
            total_daily_bnkr: total_bnkr,
            total_daily_usd: total_bnkr * self.bnkr_usd_rate,
            cache_hit_rate: (exact_dup_rate + semantic_dup_rate) * 100.0,
            drop_rate: (exact_dup_rate + semantic_dup_rate + spam_rate) * 100.0,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RevenueSimulation {
    pub total_calls: u64,
    pub total_agents: u64,
    pub base_toll_revenue_bnkr: f64,
    pub novelty_premium_revenue_bnkr: f64,
    pub new_topic_revenue_bnkr: f64,
    pub total_daily_bnkr: f64,
    pub total_daily_usd: f64,
    pub cache_hit_rate: f64,
    pub drop_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_free_tier() {
        let engine = TariffEngine::default();
        let q = engine.calculate(0.5, true, false);
        assert_eq!(q.total_bnkr, 0.0);
        assert_eq!(q.tier, PriceTier::Free);
    }

    #[test]
    fn test_duplicate_base_toll_only() {
        let engine = TariffEngine::default();
        let q = engine.calculate(0.05, false, false); // G < 0.10
        assert_eq!(q.novelty_premium_bnkr, 0.0);
        assert_eq!(q.tier, PriceTier::BaseTollOnly);
    }

    #[test]
    fn test_novel_delta_pricing() {
        let engine = TariffEngine::default();
        let q = engine.calculate(0.50, false, false);
        assert!(q.novelty_premium_bnkr > 0.0);
        assert!(q.total_bnkr > engine.base_toll);
    }

    #[test]
    fn test_new_topic_premium() {
        let engine = TariffEngine::default();
        let q = engine.calculate(1.0, false, true);
        assert_eq!(q.total_bnkr, 0.05);
        assert_eq!(q.tier, PriceTier::Premium);
    }

    #[test]
    fn test_revenue_simulation_1000_agents() {
        let engine = TariffEngine::default();
        let sim = engine.simulate_daily_revenue(1000, 1000);
        assert!(sim.total_daily_usd > 0.0);
        println!("1K 에이전트 일 수익: ${:.2}", sim.total_daily_usd);
    }
}
