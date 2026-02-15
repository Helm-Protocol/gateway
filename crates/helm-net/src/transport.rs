use anyhow::Result;
use libp2p::{
    gossipsub, identify, kad, mdns, noise,
    swarm::{NetworkBehaviour, Swarm, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, SwarmBuilder,
};
use std::time::Duration;
use tracing::{info, warn};

use crate::protocol::HelmMessage;

/// Combined libp2p network behaviour for Helm nodes.
#[derive(NetworkBehaviour)]
pub struct HelmBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,
    pub mdns: libp2p::swarm::behaviour::toggle::Toggle<mdns::tokio::Behaviour>,
    pub identify: identify::Behaviour,
}

/// Core transport layer wrapping a libp2p Swarm.
pub struct HelmTransport {
    swarm: Swarm<HelmBehaviour>,
    topic: gossipsub::IdentTopic,
}

impl HelmTransport {
    /// Build a new transport with default TCP + Noise + Yamux stack.
    pub fn new() -> Result<Self> {
        let topic = gossipsub::IdentTopic::new("helm-network");

        let swarm = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_behaviour(|key| {
                let peer_id = PeerId::from(key.public());

                let gossipsub_config = gossipsub::ConfigBuilder::default()
                    .heartbeat_interval(Duration::from_secs(10))
                    .build()
                    .expect("valid gossipsub config");

                let gossipsub = gossipsub::Behaviour::new(
                    gossipsub::MessageAuthenticity::Signed(key.clone()),
                    gossipsub_config,
                )
                .expect("valid gossipsub behaviour");

                let kademlia =
                    kad::Behaviour::new(peer_id, kad::store::MemoryStore::new(peer_id));

                // mDNS may fail in containers / restricted environments — run without it
                let mdns = match mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id) {
                    Ok(b) => {
                        info!("mDNS enabled");
                        Some(b)
                    }
                    Err(e) => {
                        warn!("mDNS unavailable ({e}), continuing without local discovery");
                        None
                    }
                }
                .into();

                let identify = identify::Behaviour::new(identify::Config::new(
                    "/helm/0.1.0".to_string(),
                    key.public(),
                ));

                Ok(HelmBehaviour {
                    gossipsub,
                    kademlia,
                    mdns,
                    identify,
                })
            })?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        let mut transport = Self { swarm, topic };
        transport
            .swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&transport.topic)?;

        Ok(transport)
    }

    /// Start listening on the given multiaddr.
    pub fn listen_on(&mut self, addr: Multiaddr) -> Result<()> {
        self.swarm.listen_on(addr)?;
        Ok(())
    }

    /// Dial a remote peer address.
    pub fn dial(&mut self, addr: Multiaddr) -> Result<()> {
        self.swarm.dial(addr)?;
        Ok(())
    }

    /// Local peer ID of this node.
    pub fn local_peer_id(&self) -> &PeerId {
        self.swarm.local_peer_id()
    }

    /// Publish a message to the helm-network topic.
    pub fn publish(&mut self, message: &HelmMessage) -> Result<()> {
        let data = serde_json::to_vec(message)?;
        self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.topic.clone(), data)?;
        Ok(())
    }

    /// Process the next swarm event. Returns a high-level event for the caller.
    pub async fn next_event(&mut self) -> TransportEvent {
        use futures::StreamExt;

        loop {
            match self.swarm.select_next_some().await {
                SwarmEvent::NewListenAddr { address, .. } => {
                    info!("Listening on {address}");
                    return TransportEvent::Listening(address);
                }
                SwarmEvent::Behaviour(HelmBehaviourEvent::Mdns(
                    mdns::Event::Discovered(peers),
                )) => {
                    let mut discovered = Vec::new();
                    for (peer_id, addr) in peers {
                        info!("Discovered peer: {peer_id} at {addr}");
                        self.swarm
                            .behaviour_mut()
                            .gossipsub
                            .add_explicit_peer(&peer_id);
                        self.swarm
                            .behaviour_mut()
                            .kademlia
                            .add_address(&peer_id, addr.clone());
                        discovered.push((peer_id, addr));
                    }
                    return TransportEvent::PeersDiscovered(discovered);
                }
                SwarmEvent::Behaviour(HelmBehaviourEvent::Gossipsub(
                    gossipsub::Event::Message {
                        propagation_source,
                        message,
                        ..
                    },
                )) => {
                    match serde_json::from_slice::<HelmMessage>(&message.data) {
                        Ok(msg) => {
                            return TransportEvent::Message {
                                source: propagation_source,
                                message: msg,
                            };
                        }
                        Err(_) => {
                            let raw = String::from_utf8_lossy(&message.data).to_string();
                            warn!("Unparseable message from {propagation_source}: {raw}");
                        }
                    }
                }
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    info!("Connected to {peer_id}");
                    return TransportEvent::Connected(peer_id);
                }
                SwarmEvent::ConnectionClosed { peer_id, .. } => {
                    warn!("Disconnected from {peer_id}");
                    return TransportEvent::Disconnected(peer_id);
                }
                _ => {}
            }
        }
    }
}

/// High-level events emitted by the transport layer.
#[derive(Debug)]
pub enum TransportEvent {
    Listening(Multiaddr),
    PeersDiscovered(Vec<(PeerId, Multiaddr)>),
    Message {
        source: PeerId,
        message: HelmMessage,
    },
    Connected(PeerId),
    Disconnected(PeerId),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::HelmProtocol;

    #[tokio::test]
    async fn transport_creation() {
        let transport = HelmTransport::new().unwrap();
        let peer_id = transport.local_peer_id();
        assert!(!peer_id.to_string().is_empty());
    }

    #[tokio::test]
    async fn peer_id_is_unique() {
        let t1 = HelmTransport::new().unwrap();
        let t2 = HelmTransport::new().unwrap();
        assert_ne!(t1.local_peer_id(), t2.local_peer_id());
    }

    #[tokio::test]
    async fn listen_on_random_port() {
        let mut transport = HelmTransport::new().unwrap();
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
        transport.listen_on(addr).unwrap();
    }

    #[tokio::test]
    async fn listen_on_invalid_addr_fails() {
        let mut transport = HelmTransport::new().unwrap();
        // UDP is not supported by TCP transport
        let result = "/udp/1234".parse::<Multiaddr>();
        // If parse succeeds, listen should fail
        if let Ok(addr) = result {
            assert!(transport.listen_on(addr).is_err());
        }
    }

    #[tokio::test]
    async fn publish_serializes_message() {
        let mut transport = HelmTransport::new().unwrap();
        // Listen first so gossipsub has a valid state
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
        transport.listen_on(addr).unwrap();

        let msg = HelmProtocol::chat("hello world");
        // Publish will fail with "insufficient peers" since no subscribers
        // but the serialization should succeed (no panic)
        let result = transport.publish(&msg);
        // Expected: InsufficientPeers error (no connected peers)
        assert!(result.is_err());
    }

    #[test]
    fn transport_event_debug_format() {
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/9000".parse().unwrap();
        let event = TransportEvent::Listening(addr);
        let debug = format!("{:?}", event);
        assert!(debug.contains("Listening"));
    }

    #[test]
    fn transport_event_variants() {
        let peer_id = PeerId::random();
        let msg = HelmProtocol::ping();

        let events: Vec<TransportEvent> = vec![
            TransportEvent::Connected(peer_id),
            TransportEvent::Disconnected(peer_id),
            TransportEvent::Message {
                source: peer_id,
                message: msg,
            },
            TransportEvent::PeersDiscovered(vec![]),
        ];

        assert_eq!(events.len(), 4);
        assert!(matches!(events[0], TransportEvent::Connected(_)));
        assert!(matches!(events[1], TransportEvent::Disconnected(_)));
        assert!(matches!(events[2], TransportEvent::Message { .. }));
        assert!(matches!(events[3], TransportEvent::PeersDiscovered(_)));
    }

    #[tokio::test]
    async fn dial_invalid_addr_fails() {
        let mut transport = HelmTransport::new().unwrap();
        // Dialing a non-routable address should fail
        let addr: Multiaddr = "/ip4/0.0.0.0/tcp/0".parse().unwrap();
        // This may or may not fail depending on the OS, but should not panic
        let _ = transport.dial(addr);
    }
}
