use anyhow::Result;
use libp2p::{
    gossipsub, identify, kad, mdns, noise,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, SwarmBuilder,
};
use std::time::Duration;
use tokio::select;
use tracing::{info, warn};

#[derive(NetworkBehaviour)]
struct HelmBehaviour {
    gossipsub: gossipsub::Behaviour,
    kademlia: kad::Behaviour<kad::store::MemoryStore>,
    mdns: mdns::tokio::Behaviour,
    identify: identify::Behaviour,
}

pub async fn run() -> Result<()> {
    let mut swarm = SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|key| {
            let peer_id = PeerId::from(key.public());

            // GossipSub for message propagation
            let gossipsub_config = gossipsub::ConfigBuilder::default()
                .heartbeat_interval(Duration::from_secs(10))
                .build()
                .expect("valid gossipsub config");
            let gossipsub = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()),
                gossipsub_config,
            )
            .expect("valid gossipsub behaviour");

            // Kademlia DHT for node discovery
            let kademlia = kad::Behaviour::new(peer_id, kad::store::MemoryStore::new(peer_id));

            // mDNS for local discovery
            let mdns = mdns::tokio::Behaviour::new(
                mdns::Config::default(),
                peer_id,
            )
            .expect("valid mdns behaviour");

            // Identify protocol
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

    // Listen on all interfaces
    let listen_addr: Multiaddr = "/ip4/0.0.0.0/tcp/0".parse()?;
    swarm.listen_on(listen_addr)?;

    info!("Local PeerID: {}", swarm.local_peer_id());

    // Subscribe to helm topic
    let topic = gossipsub::IdentTopic::new("helm-network");
    swarm.behaviour_mut().gossipsub.subscribe(&topic)?;

    info!("Node started. Listening for peers...");

    loop {
        select! {
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        info!("Listening on {address}");
                    }
                    SwarmEvent::Behaviour(HelmBehaviourEvent::Mdns(mdns::Event::Discovered(peers))) => {
                        for (peer_id, addr) in peers {
                            info!("Discovered peer: {peer_id} at {addr}");
                            swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                            swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
                        }
                    }
                    SwarmEvent::Behaviour(HelmBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                        propagation_source,
                        message,
                        ..
                    })) => {
                        let msg = String::from_utf8_lossy(&message.data);
                        info!("Message from {propagation_source}: {msg}");
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        info!("Connected to {peer_id}");
                    }
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        warn!("Disconnected from {peer_id}");
                    }
                    _ => {}
                }
            }
        }
    }
}
