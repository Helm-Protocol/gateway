-- migrations/001_init.sql
-- Helm-sense Gateway 초기 스키마
-- PostgreSQL 15+

-- ============================
-- DID Visa 테이블
-- ============================
CREATE TABLE IF NOT EXISTS local_visas (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    local_did       TEXT NOT NULL UNIQUE,   -- did:helm:agent_ULID
    global_did      TEXT NOT NULL UNIQUE,   -- did:ethr:0xABC...
    balance_bnkr    DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    reputation_score INTEGER NOT NULL DEFAULT 100,
    g_score_avg     DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    total_calls     BIGINT NOT NULL DEFAULT 0,
    total_paid_bnkr DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_active_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 글로벌 DID 조회 인덱스
CREATE INDEX IF NOT EXISTS idx_visas_global_did ON local_visas(global_did);
CREATE INDEX IF NOT EXISTS idx_visas_local_did  ON local_visas(local_did);

-- ============================
-- x402 Payment 티켓 로그
-- ============================
CREATE TABLE IF NOT EXISTS payment_tickets (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_did   TEXT NOT NULL,              -- local_did
    amount_micro BIGINT NOT NULL,           -- micro-BNKR (×10^6)
    nonce       BIGINT NOT NULL,
    ticket_hash BYTEA NOT NULL UNIQUE,      -- 재사용 방지
    settled     BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tickets_agent ON payment_tickets(agent_did);
CREATE INDEX IF NOT EXISTS idx_tickets_settled ON payment_tickets(settled) WHERE NOT settled;

-- ============================
-- Batch Settlement 로그
-- ============================
CREATE TABLE IF NOT EXISTS settlement_batches (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    merkle_root     BYTEA NOT NULL UNIQUE,
    total_bnkr      DOUBLE PRECISION NOT NULL,
    ticket_count    INTEGER NOT NULL,
    tx_hash         TEXT,                   -- Base Chain 트랜잭션
    settled_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================
-- API 호출 로그 (분석용)
-- ============================
CREATE TABLE IF NOT EXISTS api_call_logs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_did       TEXT NOT NULL,
    category        TEXT NOT NULL,          -- llm/search/defi/identity
    g_score         REAL,
    charged_bnkr    DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    cache_hit       BOOLEAN NOT NULL DEFAULT FALSE,
    latency_ms      INTEGER,
    tokens_saved    BIGINT,
    called_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 파티션 (월별 자동 아카이브)
CREATE INDEX IF NOT EXISTS idx_logs_agent ON api_call_logs(agent_did, called_at DESC);
CREATE INDEX IF NOT EXISTS idx_logs_category ON api_call_logs(category, called_at DESC);

-- ============================
-- Nonce 중복 방지 (TTL 24h)
-- ============================
CREATE TABLE IF NOT EXISTS used_nonces (
    nonce_hash  TEXT PRIMARY KEY,
    expires_at  TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '24 hours')
);

-- 자동 만료 정리 (pg_cron 또는 주기적 DELETE)
CREATE INDEX IF NOT EXISTS idx_nonces_expires ON used_nonces(expires_at);

-- ============================
-- 요약 뷰 (수익 대시보드)
-- ============================
CREATE OR REPLACE VIEW revenue_summary AS
SELECT
    DATE(called_at) AS date,
    category,
    COUNT(*)                                AS total_calls,
    SUM(CASE WHEN cache_hit THEN 1 ELSE 0 END) AS cache_hits,
    SUM(charged_bnkr)                       AS total_bnkr,
    AVG(g_score)                            AS avg_g_score,
    SUM(tokens_saved)                       AS total_tokens_saved,
    AVG(latency_ms)                         AS avg_latency_ms
FROM api_call_logs
GROUP BY DATE(called_at), category
ORDER BY date DESC;
