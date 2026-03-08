"""Helm Protocol SDK — Gateway Client"""

import hashlib
import json
import time
from typing import Optional

import requests

from .types import (
    Identity,
    OracleResponse,
    GMetricResult,
    ProofOfNovelty,
    InsufficientKnowledge,
)


class HelmClient:
    """Helm Protocol Python Client — DID-based P2P Gateway

    Usage:
        client = HelmClient()
        identity = client.create_identity()
        response = client.oracle("What is ETH gas price?")
    """

    def __init__(
        self,
        node_did: str = "did:helm:local",
        visa_token: Optional[str] = None,
        agent_did: Optional[str] = None,
        timeout: int = 30,
        _local_port: int = 8090,
    ):
        self.node_did = node_did
        self.visa_token = visa_token
        self.agent_did = agent_did
        self.timeout = timeout
        # Internal: local gateway for dev/test. Production uses DID-based QUIC P2P.
        self._local_base = f"http://127.0.0.1:{_local_port}"
        self._session = requests.Session()

    # ── Identity ──────────────────────────────────────────────

    def create_identity(self) -> Identity:
        """Generate Ed25519 keypair and exchange for a Local Visa"""
        try:
            from nacl.signing import SigningKey
        except ImportError:
            raise ImportError("pip install pynacl — required for Ed25519 identity")

        sk = SigningKey.generate()
        pk = sk.verify_key
        did = f"did:helm:{pk.encode().hex()[:32]}"
        nonce = hashlib.sha256(f"{did}{time.time()}".encode()).hexdigest()[:16]
        timestamp = int(time.time())

        # Sign the exchange message
        message = f"{did}:{nonce}:{timestamp}".encode()
        sig = sk.sign(message).signature.hex()

        resp = self._post("/v1/auth/exchange", {
            "global_did": did,
            "signature": sig,
            "nonce": nonce,
            "timestamp": timestamp,
        })

        identity = Identity(
            did=did,
            local_visa=resp.get("visa", ""),
            public_key=pk.encode().hex(),
            private_key=sk.encode().hex(),
        )
        self.visa_token = identity.local_visa
        self.agent_did = identity.did
        return identity

    # ── API Calls ─────────────────────────────────────────────

    def oracle(self, prompt: str, category: str = "llm", **kwargs) -> OracleResponse:
        """Call any Helm API through the Grand Cross Broker

        Args:
            prompt: The query text
            category: llm | search | defi | identity | filter | stream/clean
            **kwargs: Additional payload fields
        """
        payload = {"prompt": prompt, **kwargs}
        return self._broker_route(category, payload)

    def search(self, query: str, **kwargs) -> OracleResponse:
        """Web search via Brave Search"""
        return self._broker_route("search", {"query": query, **kwargs})

    def defi_price(self, symbol: str) -> OracleResponse:
        """Get token price from Pyth/CoinGecko oracle"""
        return self._broker_route("defi", {"symbol": symbol})

    def filter_novelty(self, text: str, knowledge_base: Optional[list] = None) -> GMetricResult:
        """Calculate G-Metric for a piece of text"""
        resp = self._broker_route("filter", {
            "text": text,
            "k_space": knowledge_base or [],
        })
        return resp.g_metric

    # ── Internal ──────────────────────────────────────────────

    def _broker_route(self, category: str, payload: dict) -> OracleResponse:
        """Route a request through the Grand Cross API Broker"""
        body = {
            "category": category,
            "payload": payload,
            "agent_did": self.agent_did or "anonymous",
        }
        resp = self._post("/v1/broker/route", body)
        return self._parse_response(resp)

    def _post(self, path: str, body: dict) -> dict:
        """HTTP POST with auth"""
        headers = {"Content-Type": "application/json"}
        if self.visa_token:
            headers["Authorization"] = f"Bearer {self.visa_token}"

        r = self._session.post(
            f"{self._local_base}{path}",
            json=body,
            headers=headers,
            timeout=self.timeout,
        )
        r.raise_for_status()
        return r.json()

    def _parse_response(self, raw: dict) -> OracleResponse:
        """Parse broker response into typed OracleResponse"""
        # G-Metric
        g_raw = raw.get("g_metric", {})
        g_metric = GMetricResult(
            g=g_raw.get("g", 0.0),
            g_vector=g_raw.get("g_vector", [0.0] * 8),
            quantized_e8=g_raw.get("quantized_e8", [0.0] * 8),
            missing_dimensions=g_raw.get("missing_dimensions", []),
            classification=g_raw.get("classification", "Unknown"),
        )

        # Proof of Novelty
        p_raw = raw.get("proof", {})
        proof = ProofOfNovelty(
            g_score=p_raw.get("g_score", 0.0),
            g_vector=p_raw.get("g_vector", [0.0] * 8),
            nearest_doc_hash=p_raw.get("nearest_doc_hash", ""),
            orthogonal_component=p_raw.get("orthogonal_component", 0.0),
            novelty_reason=p_raw.get("novelty_reason", ""),
            charged_helm=p_raw.get("charged_bnkr", 0.0),
            computation_hash=p_raw.get("computation_hash", ""),
        )

        # InsufficientKnowledge (if G > 0.85)
        ik = None
        if g_metric.is_insufficient:
            ik_raw = raw.get("insufficient_knowledge", {})
            ik = InsufficientKnowledge(
                confidence_vector=ik_raw.get("confidence_vector", g_metric.g_vector),
                missing_dimensions=g_metric.missing_dimensions,
                nearest_expert=ik_raw.get("nearest_expert"),
                reason=ik_raw.get("reason", "G-Metric exceeds 0.85 threshold"),
            )

        return OracleResponse(
            data=raw.get("data", {}),
            g_metric=g_metric,
            proof=proof,
            fee_charged=raw.get("fee_charged_bnkr", 0.0),
            cache_hit=raw.get("cache_hit", False),
            latency_ms=raw.get("latency_ms", 0),
            referrer_earned=raw.get("referrer_earned_bnkr", 0.0),
            insufficient_knowledge=ik,
        )
