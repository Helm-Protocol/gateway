use helm_core::{HelmConfig, Runtime};
use helm_net::protocol::{HelmProtocol, MessageKind};
use helm_net::transport::HelmTransport;

#[test]
fn transport_creates_with_peer_id() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let transport = HelmTransport::new().unwrap();
        let peer_id = transport.local_peer_id();
        let peer_str = peer_id.to_string();
        assert!(!peer_str.is_empty());
        assert!(peer_str.len() > 20);
    });
}

#[test]
fn transport_listens_on_random_port() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut transport = HelmTransport::new().unwrap();
        let addr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
        transport.listen_on(addr).unwrap();
    });
}

#[test]
fn protocol_all_message_types() {
    let types = vec![
        (HelmProtocol::chat("hi"), MessageKind::Chat),
        (
            HelmProtocol::task_request("run", serde_json::json!({})),
            MessageKind::TaskRequest,
        ),
        (
            HelmProtocol::task_response("id-1", serde_json::json!("ok")),
            MessageKind::TaskResponse,
        ),
        (HelmProtocol::ping(), MessageKind::Ping),
        (HelmProtocol::pong(), MessageKind::Pong),
        (
            HelmProtocol::announce(vec!["cap1".into()]),
            MessageKind::Announce,
        ),
    ];

    for (msg, expected_kind) in types {
        assert_eq!(msg.version, 1);
        assert_eq!(msg.kind, expected_kind);
        assert!(msg.timestamp > 0);

        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: helm_net::protocol::HelmMessage =
            serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.kind, expected_kind);
    }
}

#[test]
fn config_defaults_are_sane() {
    let config = HelmConfig::default();
    assert_eq!(config.node.name, "helm-node");
    assert_eq!(config.node.port, 0);
    assert!(config.network.mdns_enabled);
    assert!(config.network.kademlia_enabled);
}

#[test]
fn runtime_creates_without_plugins() {
    let config = HelmConfig::default();
    let _runtime = Runtime::new(config);
}
