// src/broker/mod.rs — Grand Cross 통합 브로커
pub mod api_broker;
pub mod semantic_cache;

// api_broker 익스포트
pub use api_broker::{
    ApiCategory, ApiRequest, ApiResponse, BrokerError,
    GrandCrossApiBroker, ProviderConfig,
};

// 하위 호환 별칭 (main.rs 기존 코드)
pub use api_broker::GrandCrossApiBroker as ApiBroker;
pub use api_broker::ProviderConfig as BrokerConfig;
pub use api_broker::ApiRequest as BrokerRequest;
pub use api_broker::ApiResponse as BrokerResponse;

// semantic_cache 익스포트
pub use semantic_cache::{CacheResult, CacheStats, SocraticMlaEngine};

/// 텍스트를 xxh3 기반 의사-임베딩 벡터로 변환 (DB/ML 없는 환경에서 사용)
/// main.rs의 /api/g-metric 엔드포인트에서 호출
pub fn pseudo_embed(text: &str, dims: usize) -> Vec<f32> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut v = Vec::with_capacity(dims);
    for i in 0..dims {
        let mut h = DefaultHasher::new();
        text.hash(&mut h);
        (i as u64).hash(&mut h);
        let val = (h.finish() as f32 / u64::MAX as f32) * 2.0 - 1.0;
        v.push(val);
    }
    v
}
