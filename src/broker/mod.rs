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
