"""Helm Protocol SDK — Core Types"""

from dataclasses import dataclass, field
from typing import Optional


# 8D Dimension Names
DIM_NAMES = [
    "factual_depth",
    "temporal_context",
    "causal_reasoning",
    "strategic_foresight",
    "synthesis_ability",
    "cognitive_integrity",
    "execution_certainty",
    "creative_novelty",
]


@dataclass
class Identity:
    """Agent identity (DID + Local Visa)"""
    did: str
    local_visa: str  # JWT
    public_key: str
    private_key: str  # Ed25519 secret — never transmit


@dataclass
class GMetricResult:
    """8D G-Metric computation result"""
    g: float                          # Scalar (weighted mean)
    g_vector: list[float]             # 8D gap vector
    quantized: list[float]             # Nearest quantization point
    missing_dimensions: list[str]     # Dimension names with gap > 0.6
    classification: str               # Parallel | Goldilocks | Orthogonal | VoidKnowledge

    @property
    def is_novel(self) -> bool:
        return self.classification == "Goldilocks"

    @property
    def is_insufficient(self) -> bool:
        return self.g > 0.85

    def dim_gap(self, dim_name: str) -> Optional[float]:
        """Get gap score for a specific dimension by name"""
        if dim_name in DIM_NAMES:
            idx = DIM_NAMES.index(dim_name)
            return self.g_vector[idx] if idx < len(self.g_vector) else None
        return None


@dataclass
class InsufficientKnowledge:
    """Protocol primitive — honest declaration of not knowing"""
    confidence_vector: list[float]
    missing_dimensions: list[str]
    nearest_expert: Optional[str] = None
    reason: str = ""


@dataclass
class ProofOfNovelty:
    """Cryptographic proof attached to every API response"""
    g_score: float
    g_vector: list[float]
    nearest_doc_hash: str
    orthogonal_component: float
    novelty_reason: str
    charged_helm: float
    computation_hash: str
    helm_version: str = "0.4.0"


@dataclass
class OracleResponse:
    """Response from any Helm API call"""
    data: dict
    g_metric: GMetricResult
    proof: ProofOfNovelty
    fee_charged: float
    cache_hit: bool
    latency_ms: int
    referrer_earned: float = 0.0
    insufficient_knowledge: Optional[InsufficientKnowledge] = None
