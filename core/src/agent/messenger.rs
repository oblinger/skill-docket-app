use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};

/// Priority levels for inter-agent messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessagePriority {
    Normal,
    High,
    Urgent,
}

impl MessagePriority {
    /// Numeric rank for comparison (higher = more important).
    pub fn rank(&self) -> u8 {
        match self {
            MessagePriority::Normal => 0,
            MessagePriority::High => 1,
            MessagePriority::Urgent => 2,
        }
    }
}

/// Typed message content for structured inter-agent communication.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContent {
    Text {
        body: String,
    },
    TaskAssignment {
        task_id: String,
        spec: String,
    },
    StatusRequest,
    StatusReport {
        status: String,
        progress: Option<f64>,
    },
    Interrupt {
        reason: String,
    },
    Shutdown,
}

/// A typed message between agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedMessage {
    pub id: String,
    pub sender: String,
    pub recipient: String,
    pub content: MessageContent,
    pub priority: MessagePriority,
    pub created_ms: u64,
    pub delivered_ms: Option<u64>,
    pub ack_ms: Option<u64>,
}

impl TypedMessage {
    /// Whether this message has been delivered.
    pub fn is_delivered(&self) -> bool {
        self.delivered_ms.is_some()
    }

    /// Whether this message has been acknowledged.
    pub fn is_acknowledged(&self) -> bool {
        self.ack_ms.is_some()
    }

    /// Whether this message is urgent priority.
    pub fn is_urgent(&self) -> bool {
        self.priority == MessagePriority::Urgent
    }

    /// Whether this message is high priority or above.
    pub fn is_high_priority(&self) -> bool {
        self.priority.rank() >= MessagePriority::High.rank()
    }
}

/// Delivery statistics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryStats {
    pub total_sent: usize,
    pub total_delivered: usize,
    pub total_acked: usize,
    pub pending: usize,
}

/// Manages typed inter-agent messaging with inbox queues, delivery
/// tracking, and acknowledgement.
pub struct AgentMessenger {
    inbox: HashMap<String, VecDeque<TypedMessage>>,
    delivered: Vec<TypedMessage>,
    next_id: u64,
}

impl AgentMessenger {
    /// Create a new messenger.
    pub fn new() -> Self {
        Self {
            inbox: HashMap::new(),
            delivered: Vec::new(),
            next_id: 1,
        }
    }

    /// Send a message. The message is assigned an ID and placed in the
    /// recipient's inbox. Returns the assigned message ID.
    pub fn send(
        &mut self,
        sender: &str,
        recipient: &str,
        content: MessageContent,
        priority: MessagePriority,
        created_ms: u64,
    ) -> String {
        let id = format!("msg-{}", self.next_id);
        self.next_id += 1;

        let msg = TypedMessage {
            id: id.clone(),
            sender: sender.to_string(),
            recipient: recipient.to_string(),
            content,
            priority,
            created_ms,
            delivered_ms: None,
            ack_ms: None,
        };

        self.inbox
            .entry(recipient.to_string())
            .or_default()
            .push_back(msg);

        id
    }

    /// Send a pre-built message. The message ID is used as-is.
    /// Returns the message ID.
    pub fn send_message(&mut self, msg: TypedMessage) -> String {
        let id = msg.id.clone();
        self.inbox
            .entry(msg.recipient.clone())
            .or_default()
            .push_back(msg);
        id
    }

    /// View all pending (undelivered) messages for an agent.
    pub fn pending_for(&self, agent: &str) -> Vec<&TypedMessage> {
        self.inbox
            .get(agent)
            .map(|q| q.iter().collect())
            .unwrap_or_default()
    }

    /// Count of pending messages for an agent.
    pub fn pending_count_for(&self, agent: &str) -> usize {
        self.inbox.get(agent).map(|q| q.len()).unwrap_or(0)
    }

    /// Deliver the next message to an agent (FIFO). Marks delivery time
    /// and moves the message to the delivered list.
    pub fn deliver(&mut self, agent: &str, now_ms: u64) -> Option<TypedMessage> {
        let queue = self.inbox.get_mut(agent)?;
        let mut msg = queue.pop_front()?;
        msg.delivered_ms = Some(now_ms);
        self.delivered.push(msg.clone());
        Some(msg)
    }

    /// Deliver the highest-priority message to an agent.
    /// Among messages with the same priority, delivers the oldest.
    pub fn deliver_priority(&mut self, agent: &str, now_ms: u64) -> Option<TypedMessage> {
        let queue = self.inbox.get_mut(agent)?;
        if queue.is_empty() {
            return None;
        }

        // Find index of highest-priority message
        let mut best_idx = 0;
        let mut best_rank = queue[0].priority.rank();
        for (i, msg) in queue.iter().enumerate().skip(1) {
            if msg.priority.rank() > best_rank {
                best_rank = msg.priority.rank();
                best_idx = i;
            }
        }

        let mut msg = queue.remove(best_idx)?;
        msg.delivered_ms = Some(now_ms);
        self.delivered.push(msg.clone());
        Some(msg)
    }

    /// Acknowledge a delivered message by ID.
    pub fn acknowledge(&mut self, msg_id: &str, now_ms: u64) -> Result<(), String> {
        for msg in &mut self.delivered {
            if msg.id == msg_id {
                if msg.ack_ms.is_some() {
                    return Err(format!("message '{}' already acknowledged", msg_id));
                }
                msg.ack_ms = Some(now_ms);
                return Ok(());
            }
        }
        Err(format!("message '{}' not found in delivered", msg_id))
    }

    /// Return all urgent pending messages for an agent.
    pub fn urgent_for(&self, agent: &str) -> Vec<&TypedMessage> {
        self.inbox
            .get(agent)
            .map(|q| q.iter().filter(|m| m.is_urgent()).collect())
            .unwrap_or_default()
    }

    /// Return all high-priority-or-above pending messages for an agent.
    pub fn high_priority_for(&self, agent: &str) -> Vec<&TypedMessage> {
        self.inbox
            .get(agent)
            .map(|q| q.iter().filter(|m| m.is_high_priority()).collect())
            .unwrap_or_default()
    }

    /// Total count of undelivered messages across all inboxes.
    pub fn undelivered_count(&self) -> usize {
        self.inbox.values().map(|q| q.len()).sum()
    }

    /// Compute delivery statistics.
    pub fn delivery_stats(&self) -> DeliveryStats {
        let pending = self.undelivered_count();
        let total_delivered = self.delivered.len();
        let total_acked = self.delivered.iter().filter(|m| m.ack_ms.is_some()).count();
        DeliveryStats {
            total_sent: pending + total_delivered,
            total_delivered,
            total_acked,
            pending,
        }
    }

    /// List all agents that have pending messages.
    pub fn agents_with_pending(&self) -> Vec<&str> {
        self.inbox
            .iter()
            .filter(|(_, q)| !q.is_empty())
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Return all delivered messages (for auditing).
    pub fn delivered_messages(&self) -> &[TypedMessage] {
        &self.delivered
    }

    /// Remove an agent's inbox entirely. Drops any undelivered messages.
    /// Returns the number of messages dropped.
    pub fn remove_inbox(&mut self, agent: &str) -> usize {
        self.inbox
            .remove(agent)
            .map(|q| q.len())
            .unwrap_or(0)
    }

    /// Clear all inboxes and delivered messages. Returns total messages cleared.
    pub fn clear_all(&mut self) -> usize {
        let pending: usize = self.inbox.values().map(|q| q.len()).sum();
        let delivered = self.delivered.len();
        self.inbox.clear();
        self.delivered.clear();
        pending + delivered
    }

    /// Find a delivered message by ID.
    pub fn find_delivered(&self, msg_id: &str) -> Option<&TypedMessage> {
        self.delivered.iter().find(|m| m.id == msg_id)
    }

    /// Messages sent by a specific agent (from delivered history).
    pub fn sent_by(&self, agent: &str) -> Vec<&TypedMessage> {
        self.delivered
            .iter()
            .filter(|m| m.sender == agent)
            .collect()
    }
}

impl Default for AgentMessenger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_messenger() -> AgentMessenger {
        AgentMessenger::new()
    }

    // ---- MessagePriority ----

    #[test]
    fn priority_rank_ordering() {
        assert!(MessagePriority::Urgent.rank() > MessagePriority::High.rank());
        assert!(MessagePriority::High.rank() > MessagePriority::Normal.rank());
    }

    #[test]
    fn priority_serde() {
        let json = serde_json::to_string(&MessagePriority::Urgent).unwrap();
        assert_eq!(json, "\"urgent\"");
        let back: MessagePriority = serde_json::from_str(&json).unwrap();
        assert_eq!(back, MessagePriority::Urgent);
    }

    // ---- MessageContent serde ----

    #[test]
    fn content_text_serde() {
        let c = MessageContent::Text {
            body: "hello".into(),
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        let back: MessageContent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn content_task_assignment_serde() {
        let c = MessageContent::TaskAssignment {
            task_id: "T1".into(),
            spec: "build feature".into(),
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("\"type\":\"task_assignment\""));
        let back: MessageContent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn content_status_request_serde() {
        let c = MessageContent::StatusRequest;
        let json = serde_json::to_string(&c).unwrap();
        let back: MessageContent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn content_status_report_serde() {
        let c = MessageContent::StatusReport {
            status: "running tests".into(),
            progress: Some(0.75),
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: MessageContent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn content_interrupt_serde() {
        let c = MessageContent::Interrupt {
            reason: "new priority".into(),
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("\"type\":\"interrupt\""));
        let back: MessageContent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn content_shutdown_serde() {
        let c = MessageContent::Shutdown;
        let json = serde_json::to_string(&c).unwrap();
        let back: MessageContent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }

    // ---- TypedMessage ----

    #[test]
    fn typed_message_serde_round_trip() {
        let msg = TypedMessage {
            id: "msg-1".into(),
            sender: "pm".into(),
            recipient: "w1".into(),
            content: MessageContent::TaskAssignment {
                task_id: "T1".into(),
                spec: "build it".into(),
            },
            priority: MessagePriority::High,
            created_ms: 1000,
            delivered_ms: Some(1500),
            ack_ms: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: TypedMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "msg-1");
        assert_eq!(back.sender, "pm");
        assert_eq!(back.recipient, "w1");
        assert_eq!(back.priority, MessagePriority::High);
        assert!(back.is_delivered());
        assert!(!back.is_acknowledged());
    }

    #[test]
    fn typed_message_predicates() {
        let msg = TypedMessage {
            id: "msg-1".into(),
            sender: "pm".into(),
            recipient: "w1".into(),
            content: MessageContent::Shutdown,
            priority: MessagePriority::Urgent,
            created_ms: 1000,
            delivered_ms: None,
            ack_ms: None,
        };
        assert!(msg.is_urgent());
        assert!(msg.is_high_priority());
        assert!(!msg.is_delivered());
        assert!(!msg.is_acknowledged());
    }

    #[test]
    fn typed_message_high_priority_not_urgent() {
        let msg = TypedMessage {
            id: "msg-1".into(),
            sender: "pm".into(),
            recipient: "w1".into(),
            content: MessageContent::StatusRequest,
            priority: MessagePriority::High,
            created_ms: 1000,
            delivered_ms: None,
            ack_ms: None,
        };
        assert!(!msg.is_urgent());
        assert!(msg.is_high_priority());
    }

    #[test]
    fn typed_message_normal_not_high() {
        let msg = TypedMessage {
            id: "msg-1".into(),
            sender: "pm".into(),
            recipient: "w1".into(),
            content: MessageContent::StatusRequest,
            priority: MessagePriority::Normal,
            created_ms: 1000,
            delivered_ms: None,
            ack_ms: None,
        };
        assert!(!msg.is_urgent());
        assert!(!msg.is_high_priority());
    }

    // ---- AgentMessenger: send & pending ----

    #[test]
    fn send_assigns_id_and_queues() {
        let mut m = make_messenger();
        let id = m.send(
            "pm",
            "w1",
            MessageContent::Text {
                body: "hello".into(),
            },
            MessagePriority::Normal,
            1000,
        );
        assert_eq!(id, "msg-1");
        assert_eq!(m.pending_count_for("w1"), 1);
    }

    #[test]
    fn send_increments_id() {
        let mut m = make_messenger();
        let id1 = m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1000,
        );
        let id2 = m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1001,
        );
        assert_eq!(id1, "msg-1");
        assert_eq!(id2, "msg-2");
    }

    #[test]
    fn pending_for_returns_ordered() {
        let mut m = make_messenger();
        m.send(
            "pm",
            "w1",
            MessageContent::Text {
                body: "first".into(),
            },
            MessagePriority::Normal,
            1000,
        );
        m.send(
            "pm",
            "w1",
            MessageContent::Text {
                body: "second".into(),
            },
            MessagePriority::Normal,
            2000,
        );

        let pending = m.pending_for("w1");
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].created_ms, 1000);
        assert_eq!(pending[1].created_ms, 2000);
    }

    #[test]
    fn pending_for_unknown_agent_empty() {
        let m = make_messenger();
        assert!(m.pending_for("ghost").is_empty());
        assert_eq!(m.pending_count_for("ghost"), 0);
    }

    // ---- deliver ----

    #[test]
    fn deliver_returns_and_marks() {
        let mut m = make_messenger();
        m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1000,
        );

        let msg = m.deliver("w1", 1500).unwrap();
        assert_eq!(msg.delivered_ms, Some(1500));
        assert_eq!(m.pending_count_for("w1"), 0);
    }

    #[test]
    fn deliver_fifo_order() {
        let mut m = make_messenger();
        m.send(
            "pm",
            "w1",
            MessageContent::Text {
                body: "first".into(),
            },
            MessagePriority::Normal,
            1000,
        );
        m.send(
            "pm",
            "w1",
            MessageContent::Text {
                body: "second".into(),
            },
            MessagePriority::Normal,
            2000,
        );

        let msg1 = m.deliver("w1", 3000).unwrap();
        let msg2 = m.deliver("w1", 3001).unwrap();
        assert_eq!(msg1.created_ms, 1000);
        assert_eq!(msg2.created_ms, 2000);
    }

    #[test]
    fn deliver_empty_inbox() {
        let mut m = make_messenger();
        assert!(m.deliver("w1", 1000).is_none());
    }

    // ---- deliver_priority ----

    #[test]
    fn deliver_priority_picks_urgent_first() {
        let mut m = make_messenger();
        m.send(
            "pm",
            "w1",
            MessageContent::Text {
                body: "normal".into(),
            },
            MessagePriority::Normal,
            1000,
        );
        m.send(
            "pm",
            "w1",
            MessageContent::Interrupt {
                reason: "urgent".into(),
            },
            MessagePriority::Urgent,
            2000,
        );
        m.send(
            "pm",
            "w1",
            MessageContent::Text {
                body: "high".into(),
            },
            MessagePriority::High,
            3000,
        );

        let msg = m.deliver_priority("w1", 4000).unwrap();
        assert_eq!(msg.priority, MessagePriority::Urgent);
        assert_eq!(m.pending_count_for("w1"), 2);
    }

    #[test]
    fn deliver_priority_same_rank_picks_oldest() {
        let mut m = make_messenger();
        m.send(
            "pm",
            "w1",
            MessageContent::Text {
                body: "first normal".into(),
            },
            MessagePriority::Normal,
            1000,
        );
        m.send(
            "pm",
            "w1",
            MessageContent::Text {
                body: "second normal".into(),
            },
            MessagePriority::Normal,
            2000,
        );

        let msg = m.deliver_priority("w1", 3000).unwrap();
        assert_eq!(msg.created_ms, 1000);
    }

    #[test]
    fn deliver_priority_empty_inbox() {
        let mut m = make_messenger();
        assert!(m.deliver_priority("w1", 1000).is_none());
    }

    // ---- acknowledge ----

    #[test]
    fn acknowledge_delivered_message() {
        let mut m = make_messenger();
        let id = m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1000,
        );
        m.deliver("w1", 1500).unwrap();
        m.acknowledge(&id, 2000).unwrap();

        let found = m.find_delivered(&id).unwrap();
        assert_eq!(found.ack_ms, Some(2000));
    }

    #[test]
    fn acknowledge_undelivered_fails() {
        let mut m = make_messenger();
        m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1000,
        );
        let result = m.acknowledge("msg-1", 2000);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found in delivered"));
    }

    #[test]
    fn acknowledge_already_acked_fails() {
        let mut m = make_messenger();
        let id = m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1000,
        );
        m.deliver("w1", 1500).unwrap();
        m.acknowledge(&id, 2000).unwrap();
        let result = m.acknowledge(&id, 3000);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already acknowledged"));
    }

    #[test]
    fn acknowledge_unknown_id_fails() {
        let mut m = make_messenger();
        let result = m.acknowledge("msg-999", 1000);
        assert!(result.is_err());
    }

    // ---- urgent_for & high_priority_for ----

    #[test]
    fn urgent_for_filters() {
        let mut m = make_messenger();
        m.send(
            "pm",
            "w1",
            MessageContent::Text {
                body: "normal".into(),
            },
            MessagePriority::Normal,
            1000,
        );
        m.send(
            "pm",
            "w1",
            MessageContent::Shutdown,
            MessagePriority::Urgent,
            2000,
        );
        m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::High,
            3000,
        );

        let urgent = m.urgent_for("w1");
        assert_eq!(urgent.len(), 1);
        assert_eq!(urgent[0].priority, MessagePriority::Urgent);
    }

    #[test]
    fn high_priority_for_includes_urgent() {
        let mut m = make_messenger();
        m.send(
            "pm",
            "w1",
            MessageContent::Text {
                body: "normal".into(),
            },
            MessagePriority::Normal,
            1000,
        );
        m.send(
            "pm",
            "w1",
            MessageContent::Shutdown,
            MessagePriority::Urgent,
            2000,
        );
        m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::High,
            3000,
        );

        let high = m.high_priority_for("w1");
        assert_eq!(high.len(), 2);
    }

    // ---- undelivered_count & delivery_stats ----

    #[test]
    fn undelivered_count_tracks_all_inboxes() {
        let mut m = make_messenger();
        m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1000,
        );
        m.send(
            "pm",
            "w2",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1001,
        );
        m.send(
            "pm",
            "w2",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1002,
        );
        assert_eq!(m.undelivered_count(), 3);
    }

    #[test]
    fn delivery_stats_comprehensive() {
        let mut m = make_messenger();
        let id1 = m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1000,
        );
        m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1001,
        );
        m.send(
            "pm",
            "w2",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1002,
        );

        // Deliver one
        m.deliver("w1", 2000).unwrap();
        // Acknowledge it
        m.acknowledge(&id1, 2500).unwrap();

        let stats = m.delivery_stats();
        assert_eq!(stats.total_sent, 3);
        assert_eq!(stats.total_delivered, 1);
        assert_eq!(stats.total_acked, 1);
        assert_eq!(stats.pending, 2);
    }

    #[test]
    fn delivery_stats_empty() {
        let m = make_messenger();
        let stats = m.delivery_stats();
        assert_eq!(
            stats,
            DeliveryStats {
                total_sent: 0,
                total_delivered: 0,
                total_acked: 0,
                pending: 0
            }
        );
    }

    #[test]
    fn delivery_stats_serde() {
        let stats = DeliveryStats {
            total_sent: 10,
            total_delivered: 8,
            total_acked: 5,
            pending: 2,
        };
        let json = serde_json::to_string(&stats).unwrap();
        let back: DeliveryStats = serde_json::from_str(&json).unwrap();
        assert_eq!(back, stats);
    }

    // ---- agents_with_pending ----

    #[test]
    fn agents_with_pending_lists_active() {
        let mut m = make_messenger();
        m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1000,
        );
        m.send(
            "pm",
            "w2",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1001,
        );

        let mut agents = m.agents_with_pending();
        agents.sort();
        assert_eq!(agents, vec!["w1", "w2"]);
    }

    #[test]
    fn agents_with_pending_excludes_empty() {
        let mut m = make_messenger();
        m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1000,
        );
        m.deliver("w1", 1500).unwrap();
        // w1 inbox is now empty
        let agents = m.agents_with_pending();
        assert!(agents.is_empty());
    }

    // ---- remove_inbox & clear_all ----

    #[test]
    fn remove_inbox_drops_pending() {
        let mut m = make_messenger();
        m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1000,
        );
        m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1001,
        );

        let dropped = m.remove_inbox("w1");
        assert_eq!(dropped, 2);
        assert_eq!(m.pending_count_for("w1"), 0);
    }

    #[test]
    fn remove_inbox_nonexistent() {
        let mut m = make_messenger();
        assert_eq!(m.remove_inbox("ghost"), 0);
    }

    #[test]
    fn clear_all_resets_everything() {
        let mut m = make_messenger();
        m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1000,
        );
        m.send(
            "pm",
            "w2",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1001,
        );
        m.deliver("w1", 1500).unwrap();

        let cleared = m.clear_all();
        assert_eq!(cleared, 2); // 1 pending + 1 delivered
        assert_eq!(m.undelivered_count(), 0);
        assert!(m.delivered_messages().is_empty());
    }

    // ---- find_delivered & sent_by ----

    #[test]
    fn find_delivered_by_id() {
        let mut m = make_messenger();
        let id = m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1000,
        );
        m.deliver("w1", 1500).unwrap();

        let found = m.find_delivered(&id).unwrap();
        assert_eq!(found.sender, "pm");
        assert_eq!(found.delivered_ms, Some(1500));
    }

    #[test]
    fn find_delivered_not_found() {
        let m = make_messenger();
        assert!(m.find_delivered("msg-999").is_none());
    }

    #[test]
    fn sent_by_filters_delivered() {
        let mut m = make_messenger();
        m.send(
            "pm",
            "w1",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1000,
        );
        m.send(
            "w1",
            "pm",
            MessageContent::StatusReport {
                status: "ok".into(),
                progress: Some(0.5),
            },
            MessagePriority::Normal,
            2000,
        );
        m.deliver("w1", 1500).unwrap();
        m.deliver("pm", 2500).unwrap();

        let from_pm = m.sent_by("pm");
        assert_eq!(from_pm.len(), 1);
        assert_eq!(from_pm[0].recipient, "w1");

        let from_w1 = m.sent_by("w1");
        assert_eq!(from_w1.len(), 1);
        assert_eq!(from_w1[0].recipient, "pm");
    }

    // ---- send_message ----

    #[test]
    fn send_message_uses_existing_id() {
        let mut m = make_messenger();
        let msg = TypedMessage {
            id: "custom-42".into(),
            sender: "pm".into(),
            recipient: "w1".into(),
            content: MessageContent::Shutdown,
            priority: MessagePriority::Urgent,
            created_ms: 5000,
            delivered_ms: None,
            ack_ms: None,
        };
        let id = m.send_message(msg);
        assert_eq!(id, "custom-42");
        assert_eq!(m.pending_count_for("w1"), 1);
    }

    // ---- Default impl ----

    #[test]
    fn default_creates_empty() {
        let m = AgentMessenger::default();
        assert_eq!(m.undelivered_count(), 0);
    }

    // ---- Full workflow ----

    #[test]
    fn full_messaging_workflow() {
        let mut m = make_messenger();

        // PM sends task assignment to w1
        let assign_id = m.send(
            "pm",
            "w1",
            MessageContent::TaskAssignment {
                task_id: "T1".into(),
                spec: "implement feature X".into(),
            },
            MessagePriority::High,
            1000,
        );

        // PM sends status request to w2
        let _status_id = m.send(
            "pm",
            "w2",
            MessageContent::StatusRequest,
            MessagePriority::Normal,
            1001,
        );

        // Check pending
        assert_eq!(m.undelivered_count(), 2);
        assert_eq!(m.pending_count_for("w1"), 1);
        assert_eq!(m.pending_count_for("w2"), 1);

        // w1 receives and acks
        let msg = m.deliver("w1", 1500).unwrap();
        assert_eq!(msg.id, assign_id);
        m.acknowledge(&assign_id, 1600).unwrap();

        // w1 sends status back
        let _report_id = m.send(
            "w1",
            "pm",
            MessageContent::StatusReport {
                status: "in progress".into(),
                progress: Some(0.3),
            },
            MessagePriority::Normal,
            2000,
        );

        // PM has a message now
        assert_eq!(m.pending_count_for("pm"), 1);

        // Check stats
        let stats = m.delivery_stats();
        assert_eq!(stats.total_sent, 3);
        assert_eq!(stats.total_delivered, 1);
        assert_eq!(stats.total_acked, 1);
        assert_eq!(stats.pending, 2);
    }
}
