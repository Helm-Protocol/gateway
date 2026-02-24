// src/filter/mod.rs
pub mod g_metric;
pub mod qkvg;

pub use g_metric::{
    cosine_similarity, normalize, orthogonal_component,
    GClass, GDecomposition, GMetricEngine, GMetricResult, SfeAnalogMetrics,
};
pub use qkvg::{
    layer1_heuristic, layer2_dedup, layer3_goldilocks, run_pipeline,
    FilterAction, FilterDecision, GoldilocksResult, GoldilocksVerdict,
    L1DropReason, L1Result, L2Result, VectorCache,
};
pub mod socratic_mla;
pub use socratic_mla::{CacheStats, GapAssessment, SocraticMlaEngine};
pub mod proof_of_novelty;
pub use proof_of_novelty::{build_proof_response, NoveltyProof};
