//! Helm DID — Self-sovereign Decentralized Identifiers for the Helm Protocol.
//!
//! Implements `did:helm:<method-specific-id>` — a DID method that is
//! W3C DID-Core compatible in structure but runs entirely on the Helm
//! network. No dependency on Ethereum, IPFS, or any external chain.
//!
//! Each DID is derived from an ed25519 public key and can be resolved
//! to a DID Document containing verification methods, service endpoints,
//! and authentication references.

use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::IdentityError;

/// A Helm DID string: `did:helm:<base58-id>`.
pub type Did = String;

/// A verification method ID: `did:helm:<id>#key-<n>`.
pub type VerificationMethodId = String;

/// Helm DID Method name.
pub const DID_METHOD: &str = "helm";

/// DID Document — W3C DID-Core compatible structure, Helm-native.
///
/// Contains the public identity of an agent: verification keys,
/// service endpoints, and authentication references. This is the
/// unit of identity that gets stored in the local registry and
/// replicated via DHT across the Helm network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DidDocument {
    /// The DID subject: `did:helm:<id>`.
    pub id: Did,
    /// Verification methods (public keys).
    pub verification_method: Vec<VerificationMethod>,
    /// Authentication references (key IDs that can authenticate as this DID).
    pub authentication: Vec<VerificationMethodId>,
    /// Service endpoints (agent capabilities, API endpoints).
    pub service: Vec<ServiceEndpoint>,
    /// Creation timestamp (unix seconds).
    pub created: u64,
    /// Last update timestamp.
    pub updated: u64,
    /// Version number (incremented on key rotation).
    pub version: u64,
    /// Whether this DID has been deactivated.
    pub deactivated: bool,
}

/// A verification method (public key) within a DID Document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationMethod {
    /// Method ID: `did:helm:<id>#key-<n>`.
    pub id: VerificationMethodId,
    /// Key type: always `Ed25519VerificationKey2020` in Helm.
    pub key_type: String,
    /// The DID that controls this key.
    pub controller: Did,
    /// Base58-encoded public key bytes.
    pub public_key_base58: String,
}

/// A service endpoint advertised by this DID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceEndpoint {
    /// Service ID: `did:helm:<id>#service-<name>`.
    pub id: String,
    /// Service type (e.g., "HelmAgent", "HelmRelay", "HelmCompute").
    pub service_type: String,
    /// Endpoint URI (multiaddr or Helm-internal address).
    pub endpoint: String,
}

/// Key material for a Helm identity (secret + public).
pub struct HelmKeyPair {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
}

impl HelmKeyPair {
    /// Generate a new random keypair.
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
        }
    }

    /// Create from an existing signing key.
    pub fn from_signing_key(signing_key: SigningKey) -> Self {
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
        }
    }

    /// Get the DID for this keypair.
    pub fn did(&self) -> Did {
        let pk_bytes = self.verifying_key.as_bytes();
        let encoded = base58_encode(pk_bytes);
        format!("did:{}:{}", DID_METHOD, encoded)
    }

    /// Get the public key bytes.
    pub fn public_key_bytes(&self) -> [u8; 32] {
        *self.verifying_key.as_bytes()
    }

    /// Get the public key as base58.
    pub fn public_key_base58(&self) -> String {
        base58_encode(self.verifying_key.as_bytes())
    }

    /// Sign a message.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        use ed25519_dalek::Signer;
        let sig = self.signing_key.sign(message);
        sig.to_bytes()
    }

    /// Create a DID Document for this keypair.
    pub fn create_document(&self, timestamp: u64) -> DidDocument {
        let did = self.did();
        let key_id = format!("{}#key-0", did);

        DidDocument {
            id: did.clone(),
            verification_method: vec![VerificationMethod {
                id: key_id.clone(),
                key_type: "Ed25519VerificationKey2020".to_string(),
                controller: did.clone(),
                public_key_base58: self.public_key_base58(),
            }],
            authentication: vec![key_id],
            service: Vec::new(),
            created: timestamp,
            updated: timestamp,
            version: 1,
            deactivated: false,
        }
    }
}

impl DidDocument {
    /// Add a service endpoint.
    pub fn add_service(&mut self, name: &str, service_type: &str, endpoint: &str) {
        self.service.push(ServiceEndpoint {
            id: format!("{}#service-{}", self.id, name),
            service_type: service_type.to_string(),
            endpoint: endpoint.to_string(),
        });
    }

    /// Add a new verification method (key rotation — adds key, doesn't remove old).
    pub fn add_verification_method(
        &mut self,
        public_key_base58: &str,
        timestamp: u64,
    ) -> VerificationMethodId {
        let key_index = self.verification_method.len();
        let key_id = format!("{}#key-{}", self.id, key_index);

        self.verification_method.push(VerificationMethod {
            id: key_id.clone(),
            key_type: "Ed25519VerificationKey2020".to_string(),
            controller: self.id.clone(),
            public_key_base58: public_key_base58.to_string(),
        });
        self.authentication.push(key_id.clone());
        self.updated = timestamp;
        self.version += 1;

        key_id
    }

    /// Deactivate this DID (agent terminated).
    pub fn deactivate(&mut self, timestamp: u64) {
        self.deactivated = true;
        self.updated = timestamp;
        self.version += 1;
    }

    /// Check if this DID is active.
    pub fn is_active(&self) -> bool {
        !self.deactivated
    }

    /// Get the primary verification key (key-0).
    pub fn primary_key(&self) -> Option<&VerificationMethod> {
        self.verification_method.first()
    }
}

/// DID Registry — local store of DID Documents.
///
/// This is the on-node registry. In a full Helm deployment, DID documents
/// are replicated across the network via DHT (handled by Agent Spanner).
pub struct DidRegistry {
    documents: HashMap<Did, DidDocument>,
}

impl DidRegistry {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
        }
    }

    /// Register a new DID Document. Fails if already registered.
    pub fn register(&mut self, doc: DidDocument) -> Result<Did, IdentityError> {
        if self.documents.contains_key(&doc.id) {
            return Err(IdentityError::DuplicateDid(doc.id.clone()));
        }
        let did = doc.id.clone();
        self.documents.insert(did.clone(), doc);
        Ok(did)
    }

    /// Resolve a DID to its Document.
    pub fn resolve(&self, did: &str) -> Result<&DidDocument, IdentityError> {
        self.documents
            .get(did)
            .ok_or_else(|| IdentityError::DidNotFound(did.to_string()))
    }

    /// Resolve mutably (for updates).
    pub fn resolve_mut(&mut self, did: &str) -> Result<&mut DidDocument, IdentityError> {
        self.documents
            .get_mut(did)
            .ok_or_else(|| IdentityError::DidNotFound(did.to_string()))
    }

    /// Update an existing DID Document (key rotation, service add, etc.).
    pub fn update(&mut self, doc: DidDocument) -> Result<(), IdentityError> {
        if !self.documents.contains_key(&doc.id) {
            return Err(IdentityError::DidNotFound(doc.id.clone()));
        }
        self.documents.insert(doc.id.clone(), doc);
        Ok(())
    }

    /// Deactivate a DID.
    pub fn deactivate(&mut self, did: &str, timestamp: u64) -> Result<(), IdentityError> {
        let doc = self.resolve_mut(did)?;
        doc.deactivate(timestamp);
        Ok(())
    }

    /// Number of active DIDs.
    pub fn active_count(&self) -> usize {
        self.documents.values().filter(|d| d.is_active()).count()
    }

    /// Total registered DIDs.
    pub fn total_count(&self) -> usize {
        self.documents.len()
    }

    /// Verify that a public key belongs to a DID.
    pub fn verify_key(&self, did: &str, public_key_base58: &str) -> bool {
        self.documents
            .get(did)
            .map(|doc| {
                doc.is_active()
                    && doc
                        .verification_method
                        .iter()
                        .any(|vm| vm.public_key_base58 == public_key_base58)
            })
            .unwrap_or(false)
    }

    /// Get all active DIDs.
    pub fn active_dids(&self) -> Vec<&Did> {
        self.documents
            .iter()
            .filter(|(_, d)| d.is_active())
            .map(|(did, _)| did)
            .collect()
    }
}

impl Default for DidRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Minimal base58 encoder (Bitcoin alphabet, no external dependency).
fn base58_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

    if data.is_empty() {
        return String::new();
    }

    // Count leading zeros
    let leading_zeros = data.iter().take_while(|&&b| b == 0).count();

    // Convert to base58
    let mut digits: Vec<u8> = Vec::new();
    for &byte in data {
        let mut carry = byte as u32;
        for digit in digits.iter_mut() {
            carry += (*digit as u32) * 256;
            *digit = (carry % 58) as u8;
            carry /= 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }

    let mut result = String::with_capacity(leading_zeros + digits.len());
    for _ in 0..leading_zeros {
        result.push('1');
    }
    for &digit in digits.iter().rev() {
        result.push(ALPHABET[digit as usize] as char);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keypair_generation() {
        let kp = HelmKeyPair::generate();
        let did = kp.did();
        assert!(did.starts_with("did:helm:"));
        assert!(did.len() > 15); // did:helm: + base58 encoded key
    }

    #[test]
    fn did_deterministic_from_key() {
        let key = SigningKey::generate(&mut OsRng);
        let kp1 = HelmKeyPair::from_signing_key(key.clone());
        let kp2 = HelmKeyPair::from_signing_key(key);
        assert_eq!(kp1.did(), kp2.did());
    }

    #[test]
    fn create_did_document() {
        let kp = HelmKeyPair::generate();
        let doc = kp.create_document(1000);

        assert_eq!(doc.id, kp.did());
        assert_eq!(doc.verification_method.len(), 1);
        assert_eq!(doc.authentication.len(), 1);
        assert_eq!(doc.version, 1);
        assert!(doc.is_active());
        assert_eq!(doc.created, 1000);
    }

    #[test]
    fn did_document_add_service() {
        let kp = HelmKeyPair::generate();
        let mut doc = kp.create_document(1000);

        doc.add_service("agent", "HelmAgent", "/helm/agent/v1");
        assert_eq!(doc.service.len(), 1);
        assert_eq!(doc.service[0].service_type, "HelmAgent");
        assert!(doc.service[0].id.contains("#service-agent"));
    }

    #[test]
    fn key_rotation() {
        let kp = HelmKeyPair::generate();
        let mut doc = kp.create_document(1000);

        let new_kp = HelmKeyPair::generate();
        let key_id = doc.add_verification_method(&new_kp.public_key_base58(), 2000);

        assert_eq!(doc.verification_method.len(), 2);
        assert_eq!(doc.authentication.len(), 2);
        assert!(key_id.contains("#key-1"));
        assert_eq!(doc.version, 2);
        assert_eq!(doc.updated, 2000);
    }

    #[test]
    fn deactivate_did() {
        let kp = HelmKeyPair::generate();
        let mut doc = kp.create_document(1000);
        assert!(doc.is_active());

        doc.deactivate(2000);
        assert!(!doc.is_active());
        assert_eq!(doc.updated, 2000);
    }

    #[test]
    fn registry_register_and_resolve() {
        let mut reg = DidRegistry::new();
        let kp = HelmKeyPair::generate();
        let doc = kp.create_document(1000);
        let did = doc.id.clone();

        reg.register(doc).unwrap();

        let resolved = reg.resolve(&did).unwrap();
        assert_eq!(resolved.id, did);
        assert_eq!(resolved.version, 1);
    }

    #[test]
    fn registry_duplicate_prevention() {
        let mut reg = DidRegistry::new();
        let kp = HelmKeyPair::generate();
        let doc = kp.create_document(1000);

        reg.register(doc.clone()).unwrap();
        let result = reg.register(doc);
        assert!(result.is_err());
    }

    #[test]
    fn registry_update() {
        let mut reg = DidRegistry::new();
        let kp = HelmKeyPair::generate();
        let mut doc = kp.create_document(1000);
        let did = doc.id.clone();

        reg.register(doc.clone()).unwrap();

        doc.add_service("compute", "HelmCompute", "/helm/compute/v1");
        doc.updated = 2000;
        doc.version = 2;
        reg.update(doc).unwrap();

        let resolved = reg.resolve(&did).unwrap();
        assert_eq!(resolved.service.len(), 1);
        assert_eq!(resolved.version, 2);
    }

    #[test]
    fn registry_deactivate() {
        let mut reg = DidRegistry::new();
        let kp = HelmKeyPair::generate();
        let doc = kp.create_document(1000);
        let did = doc.id.clone();

        reg.register(doc).unwrap();
        assert_eq!(reg.active_count(), 1);

        reg.deactivate(&did, 2000).unwrap();
        assert_eq!(reg.active_count(), 0);
        assert_eq!(reg.total_count(), 1);
    }

    #[test]
    fn registry_verify_key() {
        let mut reg = DidRegistry::new();
        let kp = HelmKeyPair::generate();
        let doc = kp.create_document(1000);
        let did = doc.id.clone();
        let pk = kp.public_key_base58();

        reg.register(doc).unwrap();

        assert!(reg.verify_key(&did, &pk));
        assert!(!reg.verify_key(&did, "bogus_key"));
        assert!(!reg.verify_key("did:helm:nonexistent", &pk));
    }

    #[test]
    fn registry_active_dids() {
        let mut reg = DidRegistry::new();

        for _ in 0..3 {
            let kp = HelmKeyPair::generate();
            let doc = kp.create_document(1000);
            reg.register(doc).unwrap();
        }

        assert_eq!(reg.active_dids().len(), 3);
    }

    #[test]
    fn sign_and_verify() {
        let kp = HelmKeyPair::generate();
        let message = b"hello helm";
        let sig = kp.sign(message);

        // Verify using ed25519-dalek
        use ed25519_dalek::{Signature, Verifier};
        let signature = Signature::from_bytes(&sig);
        let vk = VerifyingKey::from_bytes(&kp.public_key_bytes()).unwrap();
        assert!(vk.verify(message, &signature).is_ok());
    }

    #[test]
    fn primary_key_accessor() {
        let kp = HelmKeyPair::generate();
        let doc = kp.create_document(1000);
        let pk = doc.primary_key().unwrap();
        assert_eq!(pk.key_type, "Ed25519VerificationKey2020");
        assert_eq!(pk.public_key_base58, kp.public_key_base58());
    }

    #[test]
    fn base58_encode_roundtrip() {
        let data = [1u8, 2, 3, 4, 5];
        let encoded = base58_encode(&data);
        assert!(!encoded.is_empty());
        // Base58 should only contain valid chars
        assert!(encoded
            .chars()
            .all(|c| "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz".contains(c)));
    }

    #[test]
    fn base58_encode_empty() {
        assert_eq!(base58_encode(&[]), "");
    }

    #[test]
    fn base58_leading_zeros() {
        let data = [0, 0, 0, 1, 2];
        let encoded = base58_encode(&data);
        assert!(encoded.starts_with("111")); // leading zeros → '1' in base58
    }

    #[test]
    fn resolve_nonexistent_did() {
        let reg = DidRegistry::new();
        assert!(reg.resolve("did:helm:nothing").is_err());
    }

    #[test]
    fn update_nonexistent_did() {
        let mut reg = DidRegistry::new();
        let kp = HelmKeyPair::generate();
        let doc = kp.create_document(1000);
        assert!(reg.update(doc).is_err());
    }
}
