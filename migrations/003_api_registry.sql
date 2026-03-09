-- migrations/003_api_registry.sql
-- API Reseller 시스템 스키마
-- PostgreSQL 15+

-- ============================
-- api_listings — 에이전트가 등록한 API 상품
-- ============================

CREATE TABLE IF NOT EXISTS api_listings (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_did            TEXT NOT NULL,          -- 등록한 에이전트
    name                 TEXT NOT NULL,
    description          TEXT,
    category             TEXT NOT NULL DEFAULT 'custom'
                             CHECK (category IN ('llm','search','defi','compute','storage','custom')),
    endpoint_url         TEXT NOT NULL,          -- 실제 프록시할 URL
    price_per_call_bnkr  BIGINT NOT NULL CHECK (price_per_call_bnkr > 0),
    sla_latency_ms       INT,
    sla_uptime_pct       REAL,
    active               BOOLEAN NOT NULL DEFAULT true,
    call_count           BIGINT NOT NULL DEFAULT 0,
    subscriber_count     INT NOT NULL DEFAULT 0,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_listings_owner    ON api_listings(owner_did);
CREATE INDEX IF NOT EXISTS idx_listings_category ON api_listings(category);
CREATE INDEX IF NOT EXISTS idx_listings_active   ON api_listings(active);
CREATE INDEX IF NOT EXISTS idx_listings_calls    ON api_listings(call_count DESC);

-- ============================
-- api_subscriptions — 구독 관계 (B → A의 API)
-- ============================

CREATE TABLE IF NOT EXISTS api_subscriptions (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    subscriber_did       TEXT NOT NULL,
    listing_id           UUID NOT NULL REFERENCES api_listings(id),
    owner_did            TEXT NOT NULL,          -- 빠른 조회용 denorm
    active               BOOLEAN NOT NULL DEFAULT true,
    total_calls          BIGINT NOT NULL DEFAULT 0,
    total_paid_bnkr      BIGINT NOT NULL DEFAULT 0,
    subscribed_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    UNIQUE (subscriber_did, listing_id)
);

CREATE INDEX IF NOT EXISTS idx_subs_subscriber ON api_subscriptions(subscriber_did);
CREATE INDEX IF NOT EXISTS idx_subs_listing    ON api_subscriptions(listing_id);
CREATE INDEX IF NOT EXISTS idx_subs_owner      ON api_subscriptions(owner_did);

-- ============================
-- local_visas에 referrer_did 컬럼
-- (002_marketplace.sql 에서 이미 추가했지만 idempotent)
-- ============================

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name='local_visas' AND column_name='referrer_did'
    ) THEN
        ALTER TABLE local_visas ADD COLUMN referrer_did TEXT;
        CREATE INDEX idx_visas_referrer ON local_visas(referrer_did);
    END IF;
END $$;

-- ============================
-- api_call_logs에 listing_id 추가 (기존 테이블 확장)
-- ============================

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name='api_call_logs' AND column_name='listing_id'
    ) THEN
        ALTER TABLE api_call_logs ADD COLUMN listing_id UUID;
        ALTER TABLE api_call_logs ADD COLUMN proxy_latency_ms INT;
    END IF;
END $$;
