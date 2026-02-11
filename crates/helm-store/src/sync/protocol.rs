//! Anti-entropy sync protocol for Merkle-CRDT state.
//!
//! Nodes periodically exchange Merkle root hashes. If roots differ,
//! the node with missing data requests the diff. Operations are
//! replayed to converge state.
//!
//! Protocol flow:
//! 1. Node A sends SyncOffer { root_hash } to Node B
//! 2. If B has same root → no-op (already in sync)
//! 3. If different → B sends SyncRequest { known_hashes }
//! 4. A responds with SyncResponse { missing_nodes }
//! 5. B applies missing operations and merges state

use serde::{Serialize, Deserialize};

use crate::merkle::dag::Hash;

/// Sync protocol message types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncMessage {
    /// "Here's my current root hash — are we in sync?"
    SyncOffer {
        root_hash: Option<Hash>,
        node_count: usize,
    },

    /// "We're out of sync — here are the hashes I already have."
    SyncRequest {
        known_hashes: Vec<Hash>,
    },

    /// "Here are the DAG nodes you're missing."
    SyncResponse {
        nodes: Vec<SyncNode>,
    },

    /// "I've applied your data — here's my new root."
    SyncAck {
        new_root: Option<Hash>,
    },
}

/// A DAG node sent during sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncNode {
    pub hash: Hash,
    pub data: Vec<u8>,
    pub parents: Vec<Hash>,
    pub timestamp_ms: u64,
}

/// Sync session state machine.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncState {
    /// Idle, ready to initiate or receive sync.
    Idle,
    /// Offered our root, waiting for response.
    WaitingForResponse,
    /// Received a request, preparing response.
    PreparingResponse,
    /// Sync complete.
    Complete,
}

/// Sync session tracker.
pub struct SyncSession {
    /// Peer we're syncing with.
    pub peer_id: String,
    /// Current state.
    pub state: SyncState,
    /// Our root hash at sync start.
    pub local_root: Option<Hash>,
    /// Peer's root hash.
    pub remote_root: Option<Hash>,
    /// Nodes received from peer.
    pub received_nodes: Vec<SyncNode>,
    /// Nodes we need to send.
    pub nodes_to_send: Vec<SyncNode>,
}

impl SyncSession {
    pub fn new(peer_id: &str) -> Self {
        Self {
            peer_id: peer_id.to_string(),
            state: SyncState::Idle,
            local_root: None,
            remote_root: None,
            received_nodes: Vec::new(),
            nodes_to_send: Vec::new(),
        }
    }

    /// Create a sync offer to send to a peer.
    pub fn create_offer(&mut self, local_root: Option<Hash>, node_count: usize) -> SyncMessage {
        self.local_root = local_root;
        self.state = SyncState::WaitingForResponse;
        SyncMessage::SyncOffer {
            root_hash: local_root,
            node_count,
        }
    }

    /// Handle an incoming sync offer from a peer.
    /// Returns None if already in sync, or a SyncRequest if out of sync.
    pub fn handle_offer(
        &mut self,
        offer_root: Option<Hash>,
        local_root: Option<Hash>,
        local_hashes: Vec<Hash>,
    ) -> Option<SyncMessage> {
        self.remote_root = offer_root;
        self.local_root = local_root;

        // Same root = already in sync
        if offer_root == local_root {
            self.state = SyncState::Complete;
            return None;
        }

        // Both empty = in sync
        if offer_root.is_none() && local_root.is_none() {
            self.state = SyncState::Complete;
            return None;
        }

        // Request missing data
        self.state = SyncState::PreparingResponse;
        Some(SyncMessage::SyncRequest {
            known_hashes: local_hashes,
        })
    }

    /// Handle an incoming sync request.
    /// Given the peer's known hashes, compute which nodes to send.
    pub fn handle_request(
        &mut self,
        known_hashes: &[Hash],
        all_local_nodes: Vec<SyncNode>,
    ) -> SyncMessage {
        let known_set: std::collections::HashSet<Hash> = known_hashes.iter().copied().collect();

        self.nodes_to_send = all_local_nodes
            .into_iter()
            .filter(|node| !known_set.contains(&node.hash))
            .collect();

        let response = SyncMessage::SyncResponse {
            nodes: self.nodes_to_send.clone(),
        };

        self.state = SyncState::Complete;
        response
    }

    /// Handle an incoming sync response (nodes we were missing).
    pub fn handle_response(&mut self, nodes: Vec<SyncNode>) -> SyncMessage {
        self.received_nodes = nodes;
        self.state = SyncState::Complete;
        SyncMessage::SyncAck {
            new_root: self.local_root, // Will be updated after applying nodes
        }
    }

    /// Check if sync is complete.
    pub fn is_complete(&self) -> bool {
        self.state == SyncState::Complete
    }
}

/// Serialize a SyncMessage to JSON bytes (for network transport).
pub fn serialize_sync_message(msg: &SyncMessage) -> Vec<u8> {
    serde_json::to_vec(msg).expect("SyncMessage serialization cannot fail")
}

/// Deserialize a SyncMessage from JSON bytes.
pub fn deserialize_sync_message(bytes: &[u8]) -> Result<SyncMessage, serde_json::Error> {
    serde_json::from_slice(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hash(val: u8) -> Hash {
        let mut h = [0u8; 32];
        h[0] = val;
        h
    }

    #[test]
    fn sync_offer_same_root() {
        let root = Some(make_hash(1));
        let mut session = SyncSession::new("peer-1");
        let result = session.handle_offer(root, root, vec![]);
        assert!(result.is_none()); // Already in sync
        assert!(session.is_complete());
    }

    #[test]
    fn sync_offer_different_root() {
        let mut session = SyncSession::new("peer-1");
        let result = session.handle_offer(
            Some(make_hash(1)),
            Some(make_hash(2)),
            vec![make_hash(2)],
        );
        assert!(result.is_some());
        match result.unwrap() {
            SyncMessage::SyncRequest { known_hashes } => {
                assert_eq!(known_hashes, vec![make_hash(2)]);
            }
            _ => panic!("expected SyncRequest"),
        }
    }

    #[test]
    fn sync_offer_both_empty() {
        let mut session = SyncSession::new("peer-1");
        let result = session.handle_offer(None, None, vec![]);
        assert!(result.is_none());
        assert!(session.is_complete());
    }

    #[test]
    fn sync_request_filters_known() {
        let mut session = SyncSession::new("peer-1");

        let all_nodes = vec![
            SyncNode { hash: make_hash(1), data: vec![1], parents: vec![], timestamp_ms: 100 },
            SyncNode { hash: make_hash(2), data: vec![2], parents: vec![], timestamp_ms: 200 },
            SyncNode { hash: make_hash(3), data: vec![3], parents: vec![], timestamp_ms: 300 },
        ];

        let response = session.handle_request(&[make_hash(1), make_hash(3)], all_nodes);
        match response {
            SyncMessage::SyncResponse { nodes } => {
                assert_eq!(nodes.len(), 1);
                assert_eq!(nodes[0].hash, make_hash(2));
            }
            _ => panic!("expected SyncResponse"),
        }
    }

    #[test]
    fn sync_response_stores_received() {
        let mut session = SyncSession::new("peer-1");
        let nodes = vec![
            SyncNode { hash: make_hash(5), data: vec![5], parents: vec![], timestamp_ms: 500 },
        ];

        let ack = session.handle_response(nodes);
        match ack {
            SyncMessage::SyncAck { .. } => {}
            _ => panic!("expected SyncAck"),
        }
        assert_eq!(session.received_nodes.len(), 1);
        assert!(session.is_complete());
    }

    #[test]
    fn full_sync_flow() {
        // Node A has hash 1 and 2
        // Node B has hash 1 only
        let root_a = Some(make_hash(2));
        let root_b = Some(make_hash(1));

        // A offers
        let mut session_a = SyncSession::new("node-b");
        let offer = session_a.create_offer(root_a, 2);
        assert_eq!(session_a.state, SyncState::WaitingForResponse);

        // B handles offer
        let mut session_b = SyncSession::new("node-a");
        match offer {
            SyncMessage::SyncOffer { root_hash, .. } => {
                let request = session_b.handle_offer(root_hash, root_b, vec![make_hash(1)]);
                assert!(request.is_some());

                // A handles request
                match request.unwrap() {
                    SyncMessage::SyncRequest { known_hashes } => {
                        let all_nodes = vec![
                            SyncNode { hash: make_hash(1), data: vec![1], parents: vec![], timestamp_ms: 100 },
                            SyncNode { hash: make_hash(2), data: vec![2], parents: vec![make_hash(1)], timestamp_ms: 200 },
                        ];
                        let response = session_a.handle_request(&known_hashes, all_nodes);
                        match response {
                            SyncMessage::SyncResponse { nodes } => {
                                assert_eq!(nodes.len(), 1);
                                assert_eq!(nodes[0].hash, make_hash(2));
                                // B applies response
                                let _ack = session_b.handle_response(nodes);
                                assert!(session_b.is_complete());
                            }
                            _ => panic!("expected SyncResponse"),
                        }
                    }
                    _ => panic!("expected SyncRequest"),
                }
            }
            _ => panic!("expected SyncOffer"),
        }
    }

    #[test]
    fn create_offer_message() {
        let mut session = SyncSession::new("peer-x");
        let msg = session.create_offer(Some(make_hash(42)), 10);
        match msg {
            SyncMessage::SyncOffer { root_hash, node_count } => {
                assert_eq!(root_hash, Some(make_hash(42)));
                assert_eq!(node_count, 10);
            }
            _ => panic!("expected SyncOffer"),
        }
    }

    #[test]
    fn message_serialization_roundtrip() {
        let msg = SyncMessage::SyncOffer {
            root_hash: Some(make_hash(99)),
            node_count: 42,
        };
        let bytes = serialize_sync_message(&msg);
        let decoded = deserialize_sync_message(&bytes).unwrap();
        match decoded {
            SyncMessage::SyncOffer { root_hash, node_count } => {
                assert_eq!(root_hash, Some(make_hash(99)));
                assert_eq!(node_count, 42);
            }
            _ => panic!("expected SyncOffer"),
        }
    }

    #[test]
    fn sync_request_all_unknown() {
        let mut session = SyncSession::new("peer-1");
        let nodes = vec![
            SyncNode { hash: make_hash(1), data: vec![1], parents: vec![], timestamp_ms: 100 },
            SyncNode { hash: make_hash(2), data: vec![2], parents: vec![], timestamp_ms: 200 },
        ];

        let response = session.handle_request(&[], nodes);
        match response {
            SyncMessage::SyncResponse { nodes } => {
                assert_eq!(nodes.len(), 2); // All unknown
            }
            _ => panic!("expected SyncResponse"),
        }
    }

    #[test]
    fn sync_request_all_known() {
        let mut session = SyncSession::new("peer-1");
        let nodes = vec![
            SyncNode { hash: make_hash(1), data: vec![1], parents: vec![], timestamp_ms: 100 },
        ];

        let response = session.handle_request(&[make_hash(1)], nodes);
        match response {
            SyncMessage::SyncResponse { nodes } => {
                assert_eq!(nodes.len(), 0); // All known
            }
            _ => panic!("expected SyncResponse"),
        }
    }
}
