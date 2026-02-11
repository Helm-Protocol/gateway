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
