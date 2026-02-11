//! Agent message passing and mailbox system.
//!
//! Provides typed messages for inter-agent communication and a
//! bounded mailbox for async message delivery.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use crate::agent::AgentId;

/// The kind of message being sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageKind {
    /// Raw data payload.
    Data { payload: Vec<u8> },
    /// Text message.
    Text { content: String },
    /// Socratic question from the Claw interceptor.
    SocraticQuery {
        gap_id: u64,
        question: String,
        context: String,
        g_metric: f32,
    },
    /// Answer to a Socratic query.
    SocraticAnswer {
        gap_id: u64,
        answer: Vec<f32>,
    },
    /// Capability request from another agent.
    CapabilityRequest {
        capability: String,
        reason: String,
    },
    /// System notification (lifecycle events, alerts).
    System { event: String },
    /// Heartbeat / keep-alive.
    Ping,
    /// Heartbeat response.
    Pong,
}

/// A message exchanged between agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    /// Unique message ID within this node.
    pub id: u64,
    /// Sender agent.
    pub from: AgentId,
    /// Recipient agent.
    pub to: AgentId,
    /// Message content.
    pub kind: MessageKind,
    /// Timestamp (tick number or milliseconds).
    pub timestamp: u64,
}

impl AgentMessage {
    pub fn new(id: u64, from: AgentId, to: AgentId, kind: MessageKind, timestamp: u64) -> Self {
        Self {
            id,
            from,
            to,
            kind,
            timestamp,
        }
    }

    /// Create a text message.
    pub fn text(id: u64, from: AgentId, to: AgentId, content: &str, ts: u64) -> Self {
        Self::new(id, from, to, MessageKind::Text { content: content.to_string() }, ts)
    }

    /// Create a data message.
    pub fn data(id: u64, from: AgentId, to: AgentId, payload: Vec<u8>, ts: u64) -> Self {
        Self::new(id, from, to, MessageKind::Data { payload }, ts)
    }

    /// Create a system notification.
    pub fn system(id: u64, to: AgentId, event: &str, ts: u64) -> Self {
        Self::new(
            id,
            AgentId::new("system"),
            to,
            MessageKind::System { event: event.to_string() },
            ts,
        )
    }
}

/// Bounded mailbox for an agent. FIFO queue with configurable capacity.
#[derive(Debug)]
pub struct Mailbox {
    queue: VecDeque<AgentMessage>,
    capacity: usize,
    total_received: u64,
    total_dropped: u64,
}

impl Mailbox {
    /// Create a mailbox with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: VecDeque::with_capacity(capacity.min(1024)),
            capacity,
            total_received: 0,
            total_dropped: 0,
        }
    }

    /// Push a message into the mailbox. Returns false if mailbox is full (message dropped).
    pub fn push(&mut self, msg: AgentMessage) -> bool {
        self.total_received += 1;
        if self.queue.len() >= self.capacity {
            self.total_dropped += 1;
            return false;
        }
        self.queue.push_back(msg);
        true
    }

    /// Pop the next message (FIFO).
    pub fn pop(&mut self) -> Option<AgentMessage> {
        self.queue.pop_front()
    }

    /// Peek at the next message without removing it.
    pub fn peek(&self) -> Option<&AgentMessage> {
        self.queue.front()
    }

    /// Drain all messages from the mailbox.
    pub fn drain_all(&mut self) -> Vec<AgentMessage> {
        self.queue.drain(..).collect()
    }

    /// Number of messages currently in the mailbox.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Is the mailbox empty?
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Is the mailbox at capacity?
    pub fn is_full(&self) -> bool {
        self.queue.len() >= self.capacity
    }

    /// Maximum capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Total messages received (including dropped).
    pub fn total_received(&self) -> u64 {
        self.total_received
    }

    /// Total messages dropped due to full mailbox.
    pub fn total_dropped(&self) -> u64 {
        self.total_dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_text_creation() {
        let msg = AgentMessage::text(
            1,
            AgentId::new("alice"),
            AgentId::new("bob"),
            "hello",
            1000,
        );
        assert_eq!(msg.id, 1);
        assert_eq!(msg.from.as_str(), "alice");
        assert_eq!(msg.to.as_str(), "bob");
        match &msg.kind {
            MessageKind::Text { content } => assert_eq!(content, "hello"),
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn message_data_creation() {
        let msg = AgentMessage::data(
            2,
            AgentId::new("sender"),
            AgentId::new("recv"),
            vec![1, 2, 3],
            500,
        );
        match &msg.kind {
            MessageKind::Data { payload } => assert_eq!(payload, &[1, 2, 3]),
            _ => panic!("expected data"),
        }
    }

    #[test]
    fn message_system_creation() {
        let msg = AgentMessage::system(3, AgentId::new("agent-x"), "started", 0);
        assert_eq!(msg.from.as_str(), "system");
        match &msg.kind {
            MessageKind::System { event } => assert_eq!(event, "started"),
            _ => panic!("expected system"),
        }
    }

    #[test]
    fn message_serialize_roundtrip() {
        let msg = AgentMessage::text(
            10,
            AgentId::new("a"),
            AgentId::new("b"),
            "test",
            999,
        );
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: AgentMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, 10);
        assert_eq!(decoded.timestamp, 999);
    }

    #[test]
    fn socratic_query_message() {
        let msg = AgentMessage::new(
            5,
            AgentId::new("claw"),
            AgentId::new("agent-1"),
            MessageKind::SocraticQuery {
                gap_id: 42,
                question: "What is the shard recovery strategy?".to_string(),
                context: "erasure coding failure".to_string(),
                g_metric: 0.65,
            },
            2000,
        );
        match &msg.kind {
            MessageKind::SocraticQuery { gap_id, g_metric, .. } => {
                assert_eq!(*gap_id, 42);
                assert!(*g_metric > 0.4);
            }
            _ => panic!("expected socratic query"),
        }
    }

    #[test]
    fn mailbox_push_pop() {
        let mut mb = Mailbox::new(10);
        let msg = AgentMessage::text(1, AgentId::new("a"), AgentId::new("b"), "hi", 0);
        assert!(mb.push(msg));
        assert_eq!(mb.len(), 1);

        let popped = mb.pop().unwrap();
        assert_eq!(popped.id, 1);
        assert!(mb.is_empty());
    }

    #[test]
    fn mailbox_fifo_order() {
        let mut mb = Mailbox::new(10);
        for i in 0..5 {
            let msg = AgentMessage::text(i, AgentId::new("a"), AgentId::new("b"), "x", i);
            mb.push(msg);
        }
        for i in 0..5 {
            let msg = mb.pop().unwrap();
            assert_eq!(msg.id, i);
        }
    }

    #[test]
    fn mailbox_capacity_enforcement() {
        let mut mb = Mailbox::new(3);
        for i in 0..3 {
            let msg = AgentMessage::text(i, AgentId::new("a"), AgentId::new("b"), "x", 0);
            assert!(mb.push(msg));
        }
        assert!(mb.is_full());

        // 4th message should be dropped
        let msg = AgentMessage::text(3, AgentId::new("a"), AgentId::new("b"), "dropped", 0);
        assert!(!mb.push(msg));
        assert_eq!(mb.len(), 3);
        assert_eq!(mb.total_dropped(), 1);
        assert_eq!(mb.total_received(), 4);
    }

    #[test]
    fn mailbox_drain_all() {
        let mut mb = Mailbox::new(10);
        for i in 0..5 {
            let msg = AgentMessage::text(i, AgentId::new("a"), AgentId::new("b"), "x", 0);
            mb.push(msg);
        }
        let all = mb.drain_all();
        assert_eq!(all.len(), 5);
        assert!(mb.is_empty());
    }

    #[test]
    fn mailbox_peek() {
        let mut mb = Mailbox::new(10);
        assert!(mb.peek().is_none());

        let msg = AgentMessage::text(7, AgentId::new("a"), AgentId::new("b"), "peek", 0);
        mb.push(msg);

        let peeked = mb.peek().unwrap();
        assert_eq!(peeked.id, 7);
        assert_eq!(mb.len(), 1); // peek doesn't remove
    }

    #[test]
    fn mailbox_stats() {
        let mut mb = Mailbox::new(2);
        assert_eq!(mb.capacity(), 2);
        assert_eq!(mb.total_received(), 0);
        assert_eq!(mb.total_dropped(), 0);

        for i in 0..4 {
            let msg = AgentMessage::text(i, AgentId::new("a"), AgentId::new("b"), "x", 0);
            mb.push(msg);
        }
        assert_eq!(mb.total_received(), 4);
        assert_eq!(mb.total_dropped(), 2);
    }

    #[test]
    fn ping_pong_messages() {
        let ping = AgentMessage::new(
            1, AgentId::new("a"), AgentId::new("b"), MessageKind::Ping, 0,
        );
        let pong = AgentMessage::new(
            2, AgentId::new("b"), AgentId::new("a"), MessageKind::Pong, 1,
        );
        assert!(matches!(ping.kind, MessageKind::Ping));
        assert!(matches!(pong.kind, MessageKind::Pong));
    }
}
