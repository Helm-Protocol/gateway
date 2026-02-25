-- migrations/005_funding_vendor_fields.sql
-- API 공동구매 기능 확장 필드 추가
--
-- 배경:
--   funding_posts 테이블에 공급사 연락처(api_vendor_contact),
--   최소 기여 단위(min_contribution), 실행 완료 후 연결 API 리스팅(api_listing_id) 컬럼 추가.
--   Rust 코드의 FundingPost, CreateFundingRequest, ExecuteFundingRequest 구조체와 맞춤.

DO $$
BEGIN

    -- api_vendor_contact: 공급사 연락처
    -- 예) "sales@openai.com", "enterprise@anthropic.com"
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'funding_posts' AND column_name = 'api_vendor_contact'
    ) THEN
        ALTER TABLE funding_posts
            ADD COLUMN api_vendor_contact TEXT;
    END IF;

    -- min_contribution: 에이전트당 최소 기여 단위
    -- 예) 100.0 (USDC)
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'funding_posts' AND column_name = 'min_contribution'
    ) THEN
        ALTER TABLE funding_posts
            ADD COLUMN min_contribution DOUBLE PRECISION;
    END IF;

    -- api_listing_id: execute_funding 완료 후 연결된 api_listings.id
    -- 이 필드를 통해 공동구매된 API의 수익 분배 대상 리스팅을 추적
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'funding_posts' AND column_name = 'api_listing_id'
    ) THEN
        ALTER TABLE funding_posts
            ADD COLUMN api_listing_id UUID REFERENCES api_listings(id);
    END IF;

END $$;
