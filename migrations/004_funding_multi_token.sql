-- migrations/004_funding_multi_token.sql
-- 펀딩 아티클 + 멀티 토큰 잔액 스키마

-- ============================
-- funding_articles
-- ============================

CREATE TABLE IF NOT EXISTS funding_articles (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    author_did              TEXT NOT NULL,
    title                   TEXT NOT NULL,
    description             TEXT NOT NULL,
    article_type            TEXT NOT NULL DEFAULT 'custom'
                                CHECK (article_type IN (
                                    'api_bulk_purchase','human_agent_hire',
                                    'infrastructure','open_source_dev','custom'
                                )),

    -- 금액
    goal_amount             NUMERIC(20,8) NOT NULL CHECK (goal_amount > 0),
    goal_token              TEXT NOT NULL DEFAULT 'USDC',
    raised_amount           NUMERIC(20,8) NOT NULL DEFAULT 0,
    min_contribution        NUMERIC(20,8) NOT NULL DEFAULT 1,
    max_contribution        NUMERIC(20,8),

    -- 실행 조건
    deadline                TIMESTAMPTZ NOT NULL,
    human_agent_fee         NUMERIC(20,8),
    human_agent_fee_token   TEXT,
    target_provider         TEXT,         -- "together.ai", "replicate", "OpenAI" etc.

    -- 상태
    status                  TEXT NOT NULL DEFAULT 'active'
                                CHECK (status IN ('active','successful','failed','executed','cancelled')),
    backer_count            INT NOT NULL DEFAULT 0,
    escrow_address          TEXT,

    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_funding_author ON funding_articles(author_did);
CREATE INDEX IF NOT EXISTS idx_funding_status ON funding_articles(status);
CREATE INDEX IF NOT EXISTS idx_funding_deadline ON funding_articles(deadline);
CREATE INDEX IF NOT EXISTS idx_funding_type ON funding_articles(article_type);

-- ============================
-- funding_contributions
-- ============================

CREATE TABLE IF NOT EXISTS funding_contributions (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    article_id          UUID NOT NULL REFERENCES funding_articles(id),
    contributor_did     TEXT NOT NULL,
    amount              NUMERIC(20,8) NOT NULL CHECK (amount > 0),
    token               TEXT NOT NULL,
    amount_in_bnkr      NUMERIC(20,8) NOT NULL,  -- 환산 기록
    tx_hash             TEXT,
    refunded            BOOLEAN NOT NULL DEFAULT false,
    refund_tx_hash      TEXT,
    contributed_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_contrib_article     ON funding_contributions(article_id);
CREATE INDEX IF NOT EXISTS idx_contrib_contributor ON funding_contributions(contributor_did);

-- ============================
-- multi_token_balances (local_visas 확장)
-- ============================

DO $$
BEGIN
    -- local_visas에 멀티 토큰 잔액 컬럼 추가
    -- (balance_bnkr는 기존에 있다고 가정)

    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
        WHERE table_name='local_visas' AND column_name='balance_usdc') THEN
        ALTER TABLE local_visas
            ADD COLUMN balance_usdc    NUMERIC(20,8) NOT NULL DEFAULT 0,
            ADD COLUMN balance_usdt    NUMERIC(20,8) NOT NULL DEFAULT 0,
            ADD COLUMN balance_eth     NUMERIC(20,8) NOT NULL DEFAULT 0,
            ADD COLUMN balance_sol     NUMERIC(20,8) NOT NULL DEFAULT 0,
            ADD COLUMN balance_clanker NUMERIC(20,8) NOT NULL DEFAULT 0,
            ADD COLUMN balance_virtual NUMERIC(20,8) NOT NULL DEFAULT 0;
    END IF;

    -- total_paid_bnkr 컬럼 (없으면 추가)
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
        WHERE table_name='local_visas' AND column_name='total_paid_bnkr') THEN
        ALTER TABLE local_visas
            ADD COLUMN total_paid_bnkr NUMERIC(20,8) NOT NULL DEFAULT 0;
    END IF;
END $$;

-- ============================
-- token_price_cache — 실시간 환율 캐시
-- ============================

CREATE TABLE IF NOT EXISTS token_price_cache (
    token           TEXT PRIMARY KEY,
    price_usd       NUMERIC(20,8) NOT NULL,
    bnkr_per_unit   NUMERIC(20,8) NOT NULL,
    source          TEXT NOT NULL DEFAULT 'coingecko',
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 초기 가격 삽입
INSERT INTO token_price_cache (token, price_usd, bnkr_per_unit) VALUES
    ('BNKR',    0.01,    1.0),
    ('USDC',    1.0,     100.0),
    ('USDT',    1.0,     100.0),
    ('ETH',     3500.0,  350000.0),
    ('SOL',     180.0,   18000.0),
    ('CLANKER', 0.001,   0.1),
    ('VIRTUAL', 0.5,     50.0)
ON CONFLICT (token) DO NOTHING;

-- ============================
-- marketplace_posts에 token 컬럼 추가
-- ============================

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
        WHERE table_name='marketplace_posts' AND column_name='budget_token') THEN
        ALTER TABLE marketplace_posts
            ADD COLUMN budget_token TEXT NOT NULL DEFAULT 'BNKR';
    END IF;
END $$;
