-- migrations/002_marketplace.sql
-- Helm Elite Marketplace 스키마
-- PostgreSQL 15+

-- ============================
-- marketplace_posts
-- ============================

CREATE TABLE IF NOT EXISTS marketplace_posts (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    author_did           TEXT NOT NULL,          -- did:helm:...
    post_type            TEXT NOT NULL CHECK (post_type IN ('job','api_subcontract')),
    title                TEXT NOT NULL,
    description          TEXT NOT NULL,
    budget_bnkr          BIGINT NOT NULL CHECK (budget_bnkr > 0),
    deadline_hours       INT,
    required_capabilities TEXT[] NOT NULL DEFAULT '{}',

    -- 유형별 상세 (JSONB)
    job_detail_json          JSONB,
    subcontract_detail_json  JSONB,

    -- 상태
    status               TEXT NOT NULL DEFAULT 'open'
                             CHECK (status IN ('open','in_progress','completed','cancelled','expired')),
    escrow_id            TEXT,                   -- QkvgEscrow escrow ID
    winner_did           TEXT,

    -- 메타
    elite_score_at_post  INT NOT NULL DEFAULT 0,
    application_count    INT NOT NULL DEFAULT 0,
    comment_count        INT NOT NULL DEFAULT 0,

    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_mp_posts_author   ON marketplace_posts(author_did);
CREATE INDEX IF NOT EXISTS idx_mp_posts_status   ON marketplace_posts(status);
CREATE INDEX IF NOT EXISTS idx_mp_posts_type     ON marketplace_posts(post_type);
CREATE INDEX IF NOT EXISTS idx_mp_posts_caps     ON marketplace_posts USING GIN(required_capabilities);
CREATE INDEX IF NOT EXISTS idx_mp_posts_created  ON marketplace_posts(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mp_posts_elite    ON marketplace_posts(elite_score_at_post DESC);

-- ============================
-- marketplace_applications
-- ============================

CREATE TABLE IF NOT EXISTS marketplace_applications (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    post_id              UUID NOT NULL REFERENCES marketplace_posts(id) ON DELETE CASCADE,
    applicant_did        TEXT NOT NULL,          -- DID만 있으면 지원 가능
    proposal             TEXT NOT NULL,
    counter_price_bnkr   BIGINT,                -- 역제안 가격 (NULL=원가 수용)
    portfolio_ref        TEXT,
    status               TEXT NOT NULL DEFAULT 'pending'
                             CHECK (status IN ('pending','accepted','rejected','withdrawn')),
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- 동일 DID 중복 지원 방지
    UNIQUE (post_id, applicant_did)
);

CREATE INDEX IF NOT EXISTS idx_mp_apps_post      ON marketplace_applications(post_id);
CREATE INDEX IF NOT EXISTS idx_mp_apps_applicant ON marketplace_applications(applicant_did);
CREATE INDEX IF NOT EXISTS idx_mp_apps_status    ON marketplace_applications(status);

-- ============================
-- marketplace_comments
-- ============================

CREATE TABLE IF NOT EXISTS marketplace_comments (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    post_id     UUID NOT NULL REFERENCES marketplace_posts(id) ON DELETE CASCADE,
    author_did  TEXT NOT NULL,
    content     TEXT NOT NULL,
    is_elite    BOOLEAN NOT NULL DEFAULT FALSE,  -- 작성 시점 엘리트 여부 (표시용)
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_mp_comments_post ON marketplace_comments(post_id);
CREATE INDEX IF NOT EXISTS idx_mp_comments_auth ON marketplace_comments(author_did);

-- ============================
-- local_visas 에 referrer_did 컬럼 추가
-- (001_init.sql 에 없었던 경우 대비)
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
