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

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity::Keypair;

    fn random_peer() -> (PeerId, Multiaddr) {
        let key = Keypair::generate_ed25519();
        let peer_id = PeerId::from(key.public());
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/9000".parse().unwrap();
        (peer_id, addr)
    }

    #[test]
    fn add_and_get_peer() {
        let mut disc = Discovery::new();
        let (pid, addr) = random_peer();
        disc.add_peer(pid, addr.clone());

        assert_eq!(disc.peer_count(), 1);
        let info = disc.get_peer(&pid).unwrap();
        assert_eq!(info.peer_id, pid);
        assert_eq!(info.addresses, vec![addr]);
    }

    #[test]
    fn duplicate_address_not_added() {
        let mut disc = Discovery::new();
        let (pid, addr) = random_peer();
        disc.add_peer(pid, addr.clone());
        disc.add_peer(pid, addr.clone());

        let info = disc.get_peer(&pid).unwrap();
        assert_eq!(info.addresses.len(), 1);
    }

    #[test]
    fn multiple_addresses_per_peer() {
        let mut disc = Discovery::new();
        let (pid, addr1) = random_peer();
        let addr2: Multiaddr = "/ip4/192.168.1.1/tcp/9001".parse().unwrap();
        disc.add_peer(pid, addr1);
        disc.add_peer(pid, addr2);

        let info = disc.get_peer(&pid).unwrap();
        assert_eq!(info.addresses.len(), 2);
    }

    #[test]
    fn remove_peer() {
        let mut disc = Discovery::new();
        let (pid, addr) = random_peer();
        disc.add_peer(pid, addr);

        disc.remove_peer(&pid);
        assert_eq!(disc.peer_count(), 0);
        assert!(disc.get_peer(&pid).is_none());
    }

    #[test]
    fn known_peers_returns_all() {
        let mut disc = Discovery::new();
        let (p1, a1) = random_peer();
        let (p2, a2) = random_peer();
        disc.add_peer(p1, a1);
        disc.add_peer(p2, a2);

        let peers = disc.known_peers();
        assert_eq!(peers.len(), 2);
    }

    #[test]
    fn prune_stale_peers() {
        let mut disc = Discovery::new();
        let (pid, addr) = random_peer();
        disc.add_peer(pid, addr);

        // Zero duration = prune everything
        disc.prune_stale(std::time::Duration::from_secs(0));
        assert_eq!(disc.peer_count(), 0);
    }

    #[test]
    fn default_is_empty() {
        let disc = Discovery::default();
        assert_eq!(disc.peer_count(), 0);
    }
}
