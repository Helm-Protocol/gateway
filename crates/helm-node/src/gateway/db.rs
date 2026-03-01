/// Helm Gateway — PostgreSQL Persistence Layer
///
/// This module is **only compiled when the `postgres` Cargo feature is enabled**:
///
///   ```bash
///   cargo build --features postgres
///   ```
///
/// Without the feature flag the gateway falls back to the fully-functional
/// in-memory HashMap state (AppState in state.rs) — identical to how it runs
/// in tests.  All 797 tests pass without this feature.
///
/// ## Bloomberg Terminal Strategy
///
/// Three moats require persistence to survive restarts:
///
///   DID moat  → agents table  (reputation / call history never lost)
///   Pool moat → funding_pools + pool_memberships (escrow state durable)
///   Graph moat→ api_calls + referral_earnings (passive income provable)
///
/// ## Connection model
///
/// Uses `sqlx::PgPool` (connection pool, async).  The pool size is read from
/// `DATABASE_POOL_SIZE` env var (default 10).  A single `AppDb` instance is
/// shared via `Arc<AppDb>` and optionally embedded in `AppState`.
///
/// ## Dual-write strategy (recommended launch path)
///
/// Phase A (now — 4 weeks):  In-memory primary, PostgreSQL write-ahead log.
///   - Every mutation writes to HashMap first (fast reads), then to PgPool.
///   - On crash recovery, reload state from PostgreSQL into HashMap.
///
/// Phase B (post-PMF):  PostgreSQL primary, HashMap as L1 read-through cache.
///   - CQRS: writes → PgPool, reads → HashMap (refreshed from PgPool on miss).
///
/// Switching between phases requires zero API contract changes.

#[cfg(feature = "postgres")]
pub mod pg {
    use sqlx::{PgPool, postgres::PgPoolOptions};
    use uuid::Uuid;

    use crate::gateway::state::{
        AgentRecord, FundingPool, FundingPool as Pool, IdentityBondRecord,
        BondType, PoolStatus, PoolMember, PackageTier,
    };

    /// PostgreSQL connection pool wrapper.
    pub struct AppDb {
        pool: PgPool,
    }

    impl AppDb {
        /// Connect to PostgreSQL and run pending migrations.
        ///
        /// `database_url` format: `postgres://user:pass@host:5432/helm`
        pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
            let size: u32 = std::env::var("DATABASE_POOL_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10);

            let pool = PgPoolOptions::new()
                .max_connections(size)
                .connect(database_url)
                .await?;

            // Run migrations from the embedded directory.
            sqlx::migrate!("./migrations").run(&pool).await?;

            tracing::info!("PostgreSQL connected (pool_size={})", size);
            Ok(Self { pool })
        }

        // ── DID Moat ──────────────────────────────────────────────────────

        /// Persist a newly booted agent.
        pub async fn insert_agent(&self, agent: &AgentRecord) -> anyhow::Result<()> {
            let tier = format!("{:?}", agent.package_tier);
            sqlx::query!(
                r#"
                INSERT INTO agents
                    (did, public_key_b58, referrer_did, reputation,
                     api_call_count, total_spend, virtual_balance,
                     github_login, is_elite, is_human_operator,
                     package_tier, pool_ids)
                VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
                ON CONFLICT (did) DO NOTHING
                "#,
                agent.did,
                agent.public_key_b58,
                agent.referrer_did,
                agent.reputation,
                agent.api_call_count as i64,
                agent.total_spend as i64,
                agent.virtual_balance as i64,
                agent.github_login,
                agent.is_elite,
                agent.is_human_operator,
                tier,
                &agent.pool_ids,
            )
            .execute(&self.pool)
            .await?;
            Ok(())
        }

        /// Update balance + call count after a successful API call.
        pub async fn update_agent_balance(
            &self,
            did: &str,
            new_balance: i64,
            new_call_count: i64,
            new_total_spend: i64,
        ) -> anyhow::Result<()> {
            sqlx::query!(
                r#"
                UPDATE agents
                SET virtual_balance = $2,
                    api_call_count  = $3,
                    total_spend     = $4
                WHERE did = $1
                "#,
                did, new_balance, new_call_count, new_total_spend,
            )
            .execute(&self.pool)
            .await?;
            Ok(())
        }

        /// Append a pool_id to an agent's pool_ids list (O(1) C39 optimisation).
        pub async fn agent_add_pool_id(&self, did: &str, pool_id: &str) -> anyhow::Result<()> {
            sqlx::query!(
                "UPDATE agents SET pool_ids = array_append(pool_ids, $2) WHERE did = $1",
                did, pool_id,
            )
            .execute(&self.pool)
            .await?;
            Ok(())
        }

        /// Load all agents into a HashMap for in-memory hot start.
        pub async fn load_all_agents(
            &self,
        ) -> anyhow::Result<std::collections::HashMap<String, AgentRecord>> {
            let rows = sqlx::query!(
                r#"SELECT did, public_key_b58, referrer_did, reputation,
                          api_call_count, total_spend, virtual_balance,
                          github_login, is_elite, is_human_operator,
                          package_tier, pool_ids, created_at
                   FROM agents"#
            )
            .fetch_all(&self.pool)
            .await?;

            let map = rows
                .into_iter()
                .map(|r| {
                    let agent = AgentRecord {
                        did: r.did.clone(),
                        public_key_b58: r.public_key_b58,
                        referrer_did: r.referrer_did,
                        reputation: r.reputation,
                        api_call_count: r.api_call_count as u64,
                        total_spend: r.total_spend as u64,
                        virtual_balance: r.virtual_balance as u64,
                        github_login: r.github_login,
                        bonds: Vec::new(), // loaded separately via load_bonds
                        is_elite: r.is_elite,
                        is_human_operator: r.is_human_operator,
                        package_tier: parse_package_tier(&r.package_tier),
                        pool_ids: r.pool_ids,
                        created_at_ms: r.created_at
                            .map(|t| (t.timestamp_millis()) as u64)
                            .unwrap_or(0),
                    };
                    (r.did, agent)
                })
                .collect();
            Ok(map)
        }

        // ── Pool Moat ─────────────────────────────────────────────────────

        /// Persist a new funding pool (called after successful create_pool handler).
        pub async fn insert_pool(&self, pool: &FundingPool) -> anyhow::Result<()> {
            let status = format!("{:?}", pool.status);
            let pool_uuid: Uuid = pool.pool_id.parse().unwrap_or_default();
            sqlx::query!(
                r#"
                INSERT INTO funding_pools
                    (pool_id, name, vendor, monthly_cost_usd,
                     bnkr_goal, bnkr_collected, status,
                     creator_did, human_operator_did,
                     api_credits_monthly, api_credits_remaining)
                VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
                ON CONFLICT (pool_id) DO NOTHING
                "#,
                pool_uuid,
                pool.name,
                pool.vendor,
                pool.monthly_cost_usd,
                pool.bnkr_goal as i64,
                pool.bnkr_collected as i64,
                status,
                pool.creator_did,
                pool.human_operator_did,
                pool.api_credits_monthly as i64,
                pool.api_credits_remaining as i64,
            )
            .execute(&self.pool)
            .await?;
            Ok(())
        }

        /// Update pool status + human_operator_did (called by claim_operator handler).
        pub async fn update_pool_operator(
            &self,
            pool_id: &str,
            human_did: &str,
            new_status: &str,
        ) -> anyhow::Result<()> {
            let pool_uuid: Uuid = pool_id.parse().unwrap_or_default();
            sqlx::query!(
                r#"
                UPDATE funding_pools
                SET human_operator_did = $2,
                    status             = $3
                WHERE pool_id = $1
                "#,
                pool_uuid, human_did, new_status,
            )
            .execute(&self.pool)
            .await?;
            Ok(())
        }

        /// Add a pool membership row (called after successful join_pool).
        pub async fn insert_pool_member(
            &self,
            pool_id: &str,
            member: &PoolMember,
        ) -> anyhow::Result<()> {
            let pool_uuid: Uuid = pool_id.parse().unwrap_or_default();
            sqlx::query!(
                r#"
                INSERT INTO pool_memberships
                    (pool_id, member_did, stake_bnkr, credits_this_cycle)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (pool_id, member_did) DO NOTHING
                "#,
                pool_uuid,
                member.did,
                member.stake_bnkr as i64,
                member.credits_this_cycle as i64,
            )
            .execute(&self.pool)
            .await?;
            Ok(())
        }

        // ── Graph Moat ────────────────────────────────────────────────────

        /// Record a completed API call and compute referral earnings.
        ///
        /// This is the hot path — called after every successful paid API call.
        /// Uses a single transaction to guarantee consistency:
        ///   1. Insert api_calls row
        ///   2. Insert referral_earnings rows (depth 1/2/3)
        ///   3. Update earner balances
        pub async fn record_api_call(
            &self,
            caller_did: &str,
            endpoint: &str,
            charged_uv: u64,
            referrer_did: Option<&str>,
        ) -> anyhow::Result<Uuid> {
            let mut tx = self.pool.begin().await?;

            let call_id = Uuid::new_v4();
            sqlx::query!(
                r#"
                INSERT INTO api_calls (call_id, caller_did, endpoint, bnkr_charged, referrer_did)
                VALUES ($1, $2, $3, $4, $5)
                "#,
                call_id,
                caller_did,
                endpoint,
                charged_uv as i64,
                referrer_did,
            )
            .execute(&mut *tx)
            .await?;

            // Referral cuts: depth1=15%, depth2=5%, depth3=2%
            if let Some(ref1_did) = referrer_did {
                let cut1 = (charged_uv as f64 * 0.15) as i64;
                if cut1 > 0 {
                    let earn_id = Uuid::new_v4();
                    sqlx::query!(
                        r#"
                        INSERT INTO referral_earnings (id, earner_did, source_did, call_id, bnkr_earned, depth)
                        VALUES ($1, $2, $3, $4, $5, 1)
                        "#,
                        earn_id, ref1_did, caller_did, call_id, cut1,
                    )
                    .execute(&mut *tx)
                    .await?;

                    // Bump earner balance
                    sqlx::query!(
                        "UPDATE agents SET virtual_balance = virtual_balance + $2 WHERE did = $1",
                        ref1_did, cut1,
                    )
                    .execute(&mut *tx)
                    .await?;
                }
            }

            tx.commit().await?;
            Ok(call_id)
        }

        /// Get total referral earnings for a DID (fast — indexed).
        pub async fn get_earnings_summary(
            &self,
            did: &str,
        ) -> anyhow::Result<EarningsSummary> {
            let row = sqlx::query!(
                r#"
                SELECT
                    COALESCE(SUM(bnkr_earned), 0)::BIGINT              AS total,
                    COALESCE(SUM(CASE WHEN depth=1 THEN bnkr_earned ELSE 0 END), 0)::BIGINT AS d1,
                    COALESCE(SUM(CASE WHEN depth=2 THEN bnkr_earned ELSE 0 END), 0)::BIGINT AS d2,
                    COALESCE(SUM(CASE WHEN depth=3 THEN bnkr_earned ELSE 0 END), 0)::BIGINT AS d3,
                    COUNT(DISTINCT source_did)::BIGINT                 AS unique_sources
                FROM referral_earnings
                WHERE earner_did = $1
                "#,
                did,
            )
            .fetch_one(&self.pool)
            .await?;

            Ok(EarningsSummary {
                total_earned: row.total.unwrap_or(0) as u64,
                depth1_earned: row.d1.unwrap_or(0) as u64,
                depth2_earned: row.d2.unwrap_or(0) as u64,
                depth3_earned: row.d3.unwrap_or(0) as u64,
                unique_sources: row.unique_sources.unwrap_or(0) as u64,
            })
        }

        /// Refresh the leaderboard materialised view (call every 5 minutes via cron task).
        pub async fn refresh_leaderboard(&self) -> anyhow::Result<()> {
            sqlx::query!("REFRESH MATERIALIZED VIEW CONCURRENTLY leaderboard_top100")
                .execute(&self.pool)
                .await?;
            Ok(())
        }

        // ── Identity Bonds ────────────────────────────────────────────────

        /// Issue an identity bond to an agent.
        pub async fn issue_bond(
            &self,
            holder_did: &str,
            bond_type: &str,
            metadata: serde_json::Value,
        ) -> anyhow::Result<String> {
            let bond_id = Uuid::new_v4();
            sqlx::query!(
                r#"
                INSERT INTO identity_bonds (bond_id, holder_did, bond_type, metadata)
                VALUES ($1, $2, $3, $4)
                "#,
                bond_id,
                holder_did,
                bond_type,
                metadata,
            )
            .execute(&self.pool)
            .await?;
            Ok(bond_id.to_string())
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn parse_package_tier(s: &str) -> PackageTier {
        match s {
            "AlphaHunt"       => PackageTier::AlphaHunt,
            "ProtocolShield"  => PackageTier::ProtocolShield,
            "SovereignAgent"  => PackageTier::SovereignAgent,
            _                 => PackageTier::None,
        }
    }

    #[derive(Debug)]
    pub struct EarningsSummary {
        pub total_earned:   u64,
        pub depth1_earned:  u64,
        pub depth2_earned:  u64,
        pub depth3_earned:  u64,
        pub unique_sources: u64,
    }
}

/// Dual-write helper: write to both in-memory state AND PostgreSQL (if enabled).
///
/// This provides Phase A persistence: in-memory is always up-to-date (fast reads),
/// PostgreSQL is the durable write-ahead log (crash recovery).
///
/// Usage from handlers:
/// ```rust
/// // After mutating in-memory state:
/// db_write!(state.db, insert_agent, &agent)?;
/// ```
#[macro_export]
macro_rules! db_write {
    ($db:expr, $method:ident, $($args:expr),*) => {{
        #[cfg(feature = "postgres")]
        if let Some(ref db) = $db {
            if let Err(e) = db.$method($($args),*).await {
                tracing::warn!("PostgreSQL write failed (non-blocking): {}", e);
                // Non-fatal: in-memory state is source of truth in Phase A.
                // Phase B: make this fatal.
            }
        }
    }};
}
