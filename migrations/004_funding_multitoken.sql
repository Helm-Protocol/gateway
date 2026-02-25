-- migrations/004_funding_multitoken.sql
-- Funding 게시글 + Multi-token 잔액 스키마

-- ============================
-- funding_posts — Elite 펀딩 게시글
-- ============================

CREATE TABLE IF NOT EXISTS funding_posts (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    author_did        TEXT NOT NULL,
    title             TEXT NOT NULL,
    description       TEXT NOT NULL,
    category          TEXT NOT NULL DEFAULT 'custom'
                          CHECK (category IN
                            ('api_pooling','human_hire','infrastructure',
                             'research','dao','custom')),
    goal_amount       DOUBLE PRECISION NOT NULL CHECK (goal_amount > 0),
    token             TEXT NOT NULL DEFAULT 'BNKR',
    raised_amount     DOUBLE PRECISION NOT NULL DEFAULT 0,
    contributor_count INT  NOT NULL DEFAULT 0,
    status            TEXT NOT NULL DEFAULT 'active'
                          CHECK (status IN
                            ('active','reached','executed','expired','cancelled')),
    deadline          TIMESTAMPTZ NOT NULL,
    execution_plan    TEXT,
    human_role        TEXT,
    hire_fee          DOUBLE PRECISION,
    hire_fee_token    TEXT DEFAULT 'USDC',
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_funding_status   ON funding_posts(status);
CREATE INDEX IF NOT EXISTS idx_funding_author   ON funding_posts(author_did);
CREATE INDEX IF NOT EXISTS idx_funding_deadline ON funding_posts(deadline);
CREATE INDEX IF NOT EXISTS idx_funding_category ON funding_posts(category);

-- ============================
-- funding_contributions
-- ============================

CREATE TABLE IF NOT EXISTS funding_contributions (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    post_id          UUID NOT NULL REFERENCES funding_posts(id),
    contributor_did  TEXT NOT NULL,
    amount           DOUBLE PRECISION NOT NULL CHECK (amount > 0),
    token            TEXT NOT NULL DEFAULT 'BNKR',
    refunded         BOOLEAN NOT NULL DEFAULT false,
    contributed_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_contrib_post        ON funding_contributions(post_id);
CREATE INDEX IF NOT EXISTS idx_contrib_contributor ON funding_contributions(contributor_did);

-- ============================
-- local_visas에 multi-token 잔액 컬럼 추가
-- ============================

DO $$
DECLARE col TEXT;
BEGIN
    FOREACH col IN ARRAY ARRAY[
        'balance_eth DOUBLE PRECISION DEFAULT 0',
        'balance_usdc DOUBLE PRECISION DEFAULT 0',
        'balance_usdt DOUBLE PRECISION DEFAULT 0',
        'balance_sol DOUBLE PRECISION DEFAULT 0',
        'balance_clanker DOUBLE PRECISION DEFAULT 0',
        'balance_virtual DOUBLE PRECISION DEFAULT 0',
        'preferred_token TEXT DEFAULT ''BNKR'''
    ] LOOP
        BEGIN
            EXECUTE format('ALTER TABLE local_visas ADD COLUMN IF NOT EXISTS %s', col);
        EXCEPTION WHEN others THEN
            NULL;
        END;
    END LOOP;
END $$;

-- ============================
-- token_transactions — 모든 토큰 거래 로그
-- ============================

CREATE TABLE IF NOT EXISTS token_transactions (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    payer_did     TEXT NOT NULL,
    receiver_did  TEXT,         -- NULL = treasury
    token         TEXT NOT NULL DEFAULT 'BNKR',
    amount        DOUBLE PRECISION NOT NULL,
    usd_value     DOUBLE PRECISION,
    tx_type       TEXT NOT NULL, -- 'api_call', 'escrow_lock', 'escrow_settle', 'funding_contribute', 'funding_refund'
    memo          TEXT,
    chain_tx_hash TEXT,         -- on-chain tx hash (optional)
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_txn_payer   ON token_transactions(payer_did);
CREATE INDEX IF NOT EXISTS idx_txn_token   ON token_transactions(token);
CREATE INDEX IF NOT EXISTS idx_txn_type    ON token_transactions(tx_type);
CREATE INDEX IF NOT EXISTS idx_txn_created ON token_transactions(created_at DESC);
