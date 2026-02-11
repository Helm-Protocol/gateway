use libp2p::{Multiaddr, PeerId};
use std::collections::HashMap;
use tracing::info;

/// Tracks known peers and their addresses.
pub struct Discovery {
    peers: HashMap<PeerId, PeerInfo>,
}

/// Metadata about a discovered peer.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub peer_id: PeerId,
    pub addresses: Vec<Multiaddr>,
    pub last_seen: std::time::Instant,
}

impl Discovery {
    pub fn new() -> Self {
        Self {
            peers: HashMap::new(),
        }
    }

    /// Register a discovered peer with its address.
    pub fn add_peer(&mut self, peer_id: PeerId, addr: Multiaddr) {
        let entry = self.peers.entry(peer_id).or_insert_with(|| PeerInfo {
            peer_id,
            addresses: Vec::new(),
            last_seen: std::time::Instant::now(),
        });
        if !entry.addresses.contains(&addr) {
            entry.addresses.push(addr);
        }
        entry.last_seen = std::time::Instant::now();
    }

    /// Remove a peer from the known set.
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.peers.remove(peer_id);
    }

    /// Get info about a specific peer.
    pub fn get_peer(&self, peer_id: &PeerId) -> Option<&PeerInfo> {
        self.peers.get(peer_id)
    }

    /// Return all known peers.
    pub fn known_peers(&self) -> Vec<&PeerInfo> {
        self.peers.values().collect()
    }

    /// Number of known peers.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Prune peers not seen within the given duration.
    pub fn prune_stale(&mut self, max_age: std::time::Duration) {
        let before = self.peers.len();
        self.peers
            .retain(|_, info| info.last_seen.elapsed() < max_age);
        let pruned = before - self.peers.len();
        if pruned > 0 {
            info!("Pruned {pruned} stale peers");
        }
    }
}

impl Default for Discovery {
    fn default() -> Self {
        Self::new()
    }
}
