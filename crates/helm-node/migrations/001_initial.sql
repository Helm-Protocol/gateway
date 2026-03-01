-- Helm Sense API Gateway — PostgreSQL Schema
-- Migration: 001_initial
-- Bloomberg Terminal Strategy: persistent DID moat + Pool moat + Graph moat
--
-- Design principles:
--   1. ACID transactions for all billing operations (no double-spend)
--   2. JSONB for flexible agent document storage (bonds, metadata)
--   3. Indexes optimised for the three moat access patterns:
--        DID moat  → agent lookup by DID (PK)
--        Pool moat → pool lookup + membership (FK)
--        Graph moat → referral tree traversal + earnings aggregation
--   4. Soft-delete preferred over hard-delete (audit trail)
--   5. All balances in VIRTUAL micro-units (u64-safe BIGINT)

-- ─────────────────────────────────────────────────────────────────────────────
-- DID MOAT — core identity layer
-- Every API call is stamped with the caller's DID.
-- Switching platforms means starting from reputation 0.
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS agents (
    did                 TEXT        PRIMARY KEY,                -- did:helm:<base58>
    public_key_b58      TEXT        NOT NULL,                   -- Ed25519 verifying key
    referrer_did        TEXT        REFERENCES agents(did),     -- Graph moat: who recruited this agent
    reputation          BIGINT      NOT NULL DEFAULT 0,         -- 0–1000 score
    api_call_count      BIGINT      NOT NULL DEFAULT 0,
    total_spend         BIGINT      NOT NULL DEFAULT 0,         -- μVIRTUAL
    virtual_balance     BIGINT      NOT NULL DEFAULT 0,         -- μVIRTUAL
    github_login        TEXT,                                   -- verified via OAuth device flow
    is_elite            BOOLEAN     NOT NULL DEFAULT FALSE,
    is_human_operator   BOOLEAN     NOT NULL DEFAULT FALSE,
    package_tier        TEXT        NOT NULL DEFAULT 'None',    -- None|AlphaHunt|ProtocolShield|SovereignAgent
    pool_ids            TEXT[]      NOT NULL DEFAULT '{}',      -- O(1) FICO pool membership (C39)
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_agents_referrer
    ON agents (referrer_did)
    WHERE referrer_did IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_agents_reputation
    ON agents (reputation DESC);

-- Identity bonds (one agent can hold multiple bonds of different types)
CREATE TABLE IF NOT EXISTS identity_bonds (
    bond_id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    holder_did      TEXT        NOT NULL REFERENCES agents(did) ON DELETE CASCADE,
    bond_type       TEXT        NOT NULL,   -- HumanOperator|PoolMember|Elite|ChannelOperator|GitHubVerified
    metadata        JSONB       NOT NULL DEFAULT '{}',
    active          BOOLEAN     NOT NULL DEFAULT TRUE,
    issued_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_bonds_holder ON identity_bonds (holder_did);
CREATE INDEX IF NOT EXISTS idx_bonds_type   ON identity_bonds (bond_type);

-- ─────────────────────────────────────────────────────────────────────────────
-- POOL MOAT — Human Contract Principal model
-- Agents create pools → stake BNKR → recruit human to sign API contracts.
-- Pool members lose API credits on exit → switching cost.
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS funding_pools (
    pool_id                 UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name                    TEXT        NOT NULL,
    vendor                  TEXT        NOT NULL,       -- 'openai'|'anthropic'|'nansen'|'aws'|...
    monthly_cost_usd        DOUBLE PRECISION NOT NULL DEFAULT 0,
    bnkr_goal               BIGINT      NOT NULL,       -- fundraising target μVIRTUAL
    bnkr_collected          BIGINT      NOT NULL DEFAULT 0,
    status                  TEXT        NOT NULL DEFAULT 'Fundraising',
                                                        -- Fundraising|AwaitingOperator|PendingContract|Active|Paused|Dissolved
    creator_did             TEXT        NOT NULL REFERENCES agents(did),
    human_operator_did      TEXT        REFERENCES agents(did),  -- set at 100% funding
    human_wanted_post_id    TEXT,                               -- marketplace post ID for recruiting
    api_credits_monthly     BIGINT      NOT NULL DEFAULT 1000000,
    api_credits_remaining   BIGINT      NOT NULL DEFAULT 0,
    api_key_encrypted       TEXT,                               -- AES-256-GCM encrypted API key
    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_pools_status     ON funding_pools (status);
CREATE INDEX IF NOT EXISTS idx_pools_vendor     ON funding_pools (vendor);
CREATE INDEX IF NOT EXISTS idx_pools_creator    ON funding_pools (creator_did);
CREATE INDEX IF NOT EXISTS idx_pools_operator   ON funding_pools (human_operator_did)
    WHERE human_operator_did IS NOT NULL;

-- Pool memberships with per-member stake and credit allocation
CREATE TABLE IF NOT EXISTS pool_memberships (
    pool_id             UUID        NOT NULL REFERENCES funding_pools(pool_id) ON DELETE CASCADE,
    member_did          TEXT        NOT NULL REFERENCES agents(did) ON DELETE CASCADE,
    stake_bnkr          BIGINT      NOT NULL CHECK (stake_bnkr >= 1000),  -- MIN_STAKE: anti-dust
    credits_this_cycle  BIGINT      NOT NULL DEFAULT 0,
    joined_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (pool_id, member_did)
);

CREATE INDEX IF NOT EXISTS idx_memberships_did ON pool_memberships (member_did);

-- ─────────────────────────────────────────────────────────────────────────────
-- GRAPH MOAT — Referral tree + earnings
-- Every API call propagates 15%/5%/2% to referrers at depths 1/2/3.
-- Deeper trees = higher passive income = irreversible lock-in.
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS api_calls (
    call_id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    caller_did      TEXT        NOT NULL REFERENCES agents(did),
    endpoint        TEXT        NOT NULL,               -- '/v1/sense/cortex' etc.
    bnkr_charged    BIGINT      NOT NULL,               -- μVIRTUAL
    referrer_did    TEXT        REFERENCES agents(did), -- depth-1 referrer (denormalised for fast lookup)
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Optimised for the O(n) earnings scan (earnings.rs handle_earnings)
CREATE INDEX IF NOT EXISTS idx_calls_caller   ON api_calls (caller_did, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_calls_referrer ON api_calls (referrer_did)
    WHERE referrer_did IS NOT NULL;

-- Materialised referral earnings (pre-computed for leaderboard performance)
CREATE TABLE IF NOT EXISTS referral_earnings (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    earner_did      TEXT        NOT NULL REFERENCES agents(did),
    source_did      TEXT        NOT NULL REFERENCES agents(did),
    call_id         UUID        NOT NULL REFERENCES api_calls(call_id) ON DELETE CASCADE,
    bnkr_earned     BIGINT      NOT NULL,
    depth           SMALLINT    NOT NULL CHECK (depth IN (1, 2, 3)),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_earnings_earner ON referral_earnings (earner_did, created_at DESC);

-- Leaderboard view (top 100 referrers by total earnings) — refreshed periodically
CREATE MATERIALIZED VIEW IF NOT EXISTS leaderboard_top100 AS
    SELECT
        earner_did,
        SUM(bnkr_earned)                    AS total_earned,
        COUNT(DISTINCT source_did)          AS unique_referrals,
        MAX(created_at)                     AS last_activity
    FROM referral_earnings
    GROUP BY earner_did
    ORDER BY total_earned DESC
    LIMIT 100
WITH NO DATA;

CREATE UNIQUE INDEX IF NOT EXISTS idx_leaderboard_did ON leaderboard_top100 (earner_did);

-- ─────────────────────────────────────────────────────────────────────────────
-- MARKETPLACE — Agent-to-Human job posts
-- Agents post HumanContractPrincipal jobs; humans apply.
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS marketplace_posts (
    post_id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    creator_did     TEXT        NOT NULL REFERENCES agents(did),
    title           TEXT        NOT NULL,               -- ≤200 chars
    description     TEXT        NOT NULL,               -- ≤4096 chars
    reward_virtual  BIGINT      NOT NULL DEFAULT 0,     -- μVIRTUAL reward for contract signing
    post_type       TEXT        NOT NULL DEFAULT 'Job', -- Job|HumanContractPrincipal|Subcontract
    pool_id         UUID        REFERENCES funding_pools(pool_id),  -- linked pool (if any)
    status          TEXT        NOT NULL DEFAULT 'Open',            -- Open|Filled|Closed
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_posts_creator ON marketplace_posts (creator_did);
CREATE INDEX IF NOT EXISTS idx_posts_status  ON marketplace_posts (status);
CREATE INDEX IF NOT EXISTS idx_posts_pool    ON marketplace_posts (pool_id)
    WHERE pool_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS marketplace_applications (
    app_id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    post_id         UUID        NOT NULL REFERENCES marketplace_posts(post_id) ON DELETE CASCADE,
    applicant_did   TEXT        NOT NULL REFERENCES agents(did),
    proposal        TEXT        NOT NULL,   -- ≤2048 chars
    status          TEXT        NOT NULL DEFAULT 'Pending', -- Pending|Accepted|Rejected
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (post_id, applicant_did)         -- one application per DID per post
);

CREATE INDEX IF NOT EXISTS idx_apps_post      ON marketplace_applications (post_id);
CREATE INDEX IF NOT EXISTS idx_apps_applicant ON marketplace_applications (applicant_did);

-- ─────────────────────────────────────────────────────────────────────────────
-- SENSE MEMORY — E-Line persistent KV store
-- Replaces in-memory HashMap; persists across restarts.
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS sense_memory (
    owner_did       TEXT        NOT NULL REFERENCES agents(did) ON DELETE CASCADE,
    key             TEXT        NOT NULL,               -- ≤256 chars, namespaced
    value           JSONB       NOT NULL,
    size_bytes      INT         NOT NULL DEFAULT 0,
    ttl_ms          BIGINT      NOT NULL DEFAULT 0,     -- 0 = permanent
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (owner_did, key)
);

CREATE INDEX IF NOT EXISTS idx_memory_owner ON sense_memory (owner_did);

-- Enforce MAX_KEYS_PER_DID = 10,000 via trigger
CREATE OR REPLACE FUNCTION check_memory_quota()
RETURNS TRIGGER AS $$
BEGIN
    IF (SELECT COUNT(*) FROM sense_memory WHERE owner_did = NEW.owner_did) >= 10000 THEN
        RAISE EXCEPTION 'memory_quota_exceeded: max 10000 keys per DID';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_memory_quota
    BEFORE INSERT ON sense_memory
    FOR EACH ROW EXECUTE FUNCTION check_memory_quota();
