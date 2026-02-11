use serde::{Deserialize, Serialize};

/// Wire-format messages exchanged between Helm nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelmMessage {
    /// Protocol version for forward compatibility.
    pub version: u8,
    /// Message type tag.
    pub kind: MessageKind,
    /// Opaque payload (JSON-encoded inner data).
    pub payload: serde_json::Value,
    /// Unix timestamp in seconds.
    pub timestamp: u64,
}

/// Discriminant for message routing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    /// Plain text broadcast.
    Chat,
    /// Agent-to-agent task delegation.
    TaskRequest,
    /// Response to a task request.
    TaskResponse,
    /// Heartbeat / keepalive.
    Ping,
    /// Acknowledgement.
    Pong,
    /// Node capability advertisement.
    Announce,
}

/// Builder for constructing messages with the current protocol version.
pub struct HelmProtocol;

impl HelmProtocol {
    pub const VERSION: u8 = 1;

    pub fn chat(text: &str) -> HelmMessage {
        HelmMessage {
            version: Self::VERSION,
            kind: MessageKind::Chat,
            payload: serde_json::json!({ "text": text }),
            timestamp: now(),
        }
    }

    pub fn task_request(task: &str, params: serde_json::Value) -> HelmMessage {
        HelmMessage {
            version: Self::VERSION,
            kind: MessageKind::TaskRequest,
            payload: serde_json::json!({ "task": task, "params": params }),
            timestamp: now(),
        }
    }

    pub fn task_response(task_id: &str, result: serde_json::Value) -> HelmMessage {
        HelmMessage {
            version: Self::VERSION,
            kind: MessageKind::TaskResponse,
            payload: serde_json::json!({ "task_id": task_id, "result": result }),
            timestamp: now(),
        }
    }

    pub fn ping() -> HelmMessage {
        HelmMessage {
            version: Self::VERSION,
            kind: MessageKind::Ping,
            payload: serde_json::Value::Null,
            timestamp: now(),
        }
    }

    pub fn pong() -> HelmMessage {
        HelmMessage {
            version: Self::VERSION,
            kind: MessageKind::Pong,
            payload: serde_json::Value::Null,
            timestamp: now(),
        }
    }

    pub fn announce(capabilities: Vec<String>) -> HelmMessage {
        HelmMessage {
            version: Self::VERSION,
            kind: MessageKind::Announce,
            payload: serde_json::json!({ "capabilities": capabilities }),
            timestamp: now(),
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_message_roundtrip() {
        let msg = HelmProtocol::chat("hello world");
        assert_eq!(msg.version, 1);
        assert_eq!(msg.kind, MessageKind::Chat);
        assert_eq!(msg.payload["text"], "hello world");
        assert!(msg.timestamp > 0);

        let json = serde_json::to_string(&msg).unwrap();
        let decoded: HelmMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.kind, MessageKind::Chat);
        assert_eq!(decoded.payload["text"], "hello world");
    }

    #[test]
    fn task_request_message() {
        let params = serde_json::json!({"model": "gpt-4", "max_tokens": 100});
        let msg = HelmProtocol::task_request("summarize", params.clone());
        assert_eq!(msg.kind, MessageKind::TaskRequest);
        assert_eq!(msg.payload["task"], "summarize");
        assert_eq!(msg.payload["params"], params);
    }

    #[test]
    fn task_response_message() {
        let result = serde_json::json!({"summary": "done"});
        let msg = HelmProtocol::task_response("task-001", result.clone());
        assert_eq!(msg.kind, MessageKind::TaskResponse);
        assert_eq!(msg.payload["task_id"], "task-001");
        assert_eq!(msg.payload["result"], result);
    }

    #[test]
    fn ping_pong_messages() {
        let ping = HelmProtocol::ping();
        assert_eq!(ping.kind, MessageKind::Ping);
        assert!(ping.payload.is_null());

        let pong = HelmProtocol::pong();
        assert_eq!(pong.kind, MessageKind::Pong);
        assert!(pong.payload.is_null());
    }

    #[test]
    fn announce_message() {
        let msg = HelmProtocol::announce(vec!["chat".into(), "task".into()]);
        assert_eq!(msg.kind, MessageKind::Announce);
        let caps = msg.payload["capabilities"].as_array().unwrap();
        assert_eq!(caps.len(), 2);
        assert_eq!(caps[0], "chat");
        assert_eq!(caps[1], "task");
    }

    #[test]
    fn message_kind_serde_snake_case() {
        let msg = HelmProtocol::task_request("x", serde_json::Value::Null);
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"task_request\""));

        let msg = HelmProtocol::chat("x");
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"chat\""));
    }

    #[test]
    fn message_binary_roundtrip() {
        let msg = HelmProtocol::chat("binary test");
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: HelmMessage = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.kind, MessageKind::Chat);
        assert_eq!(decoded.payload["text"], "binary test");
    }
}
