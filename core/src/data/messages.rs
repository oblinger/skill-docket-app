use crate::types::message::Message;


/// In-memory store for agent-to-agent messages with per-recipient FIFO delivery.
#[derive(Debug, Clone)]
pub struct MessageStore {
    messages: Vec<Message>,
}


impl MessageStore {
    /// Create an empty store.
    pub fn new() -> Self {
        MessageStore {
            messages: Vec::new(),
        }
    }

    /// Enqueue a message. It should have `delivered_at_ms = None`.
    pub fn enqueue(&mut self, msg: Message) {
        self.messages.push(msg);
    }

    /// Return references to all pending (undelivered) messages for a given agent.
    pub fn pending_for(&self, agent: &str) -> Vec<&Message> {
        self.messages
            .iter()
            .filter(|m| m.recipient == agent && m.delivered_at_ms.is_none())
            .collect()
    }

    /// Deliver the oldest pending message for the given agent (FIFO).
    /// Marks `delivered_at_ms` using the provided timestamp function and returns
    /// a clone of the delivered message.
    pub fn deliver(&mut self, agent: &str) -> Option<Message> {
        let pos = self
            .messages
            .iter()
            .position(|m| m.recipient == agent && m.delivered_at_ms.is_none())?;
        // Mark as delivered with a simple timestamp (milliseconds since we don't
        // have a clock dependency, we use a sentinel value; callers can overwrite).
        self.messages[pos].delivered_at_ms = Some(now_ms());
        Some(self.messages[pos].clone())
    }

    /// Return references to all pending messages across all agents.
    pub fn all_pending(&self) -> Vec<&Message> {
        self.messages
            .iter()
            .filter(|m| m.delivered_at_ms.is_none())
            .collect()
    }

    /// Total number of messages (delivered and pending).
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}


impl Default for MessageStore {
    fn default() -> Self {
        Self::new()
    }
}


/// Simple wall-clock milliseconds. Uses `SystemTime` from std; no external deps.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}


#[cfg(test)]
mod tests {
    use super::*;

    fn make_msg(sender: &str, recipient: &str, text: &str) -> Message {
        Message {
            sender: sender.into(),
            recipient: recipient.into(),
            text: text.into(),
            queued_at_ms: 1700000000000,
            delivered_at_ms: None,
        }
    }

    #[test]
    fn new_store_is_empty() {
        let store = MessageStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn enqueue_and_pending() {
        let mut store = MessageStore::new();
        store.enqueue(make_msg("pm", "w1", "hello"));
        store.enqueue(make_msg("pm", "w2", "world"));
        assert_eq!(store.pending_for("w1").len(), 1);
        assert_eq!(store.pending_for("w2").len(), 1);
        assert_eq!(store.pending_for("w3").len(), 0);
    }

    #[test]
    fn deliver_returns_oldest_first() {
        let mut store = MessageStore::new();
        store.enqueue(make_msg("pm", "w1", "first"));
        store.enqueue(make_msg("pm", "w1", "second"));
        let delivered = store.deliver("w1").unwrap();
        assert_eq!(delivered.text, "first");
        assert!(delivered.delivered_at_ms.is_some());

        // Next delivery should be "second"
        let delivered2 = store.deliver("w1").unwrap();
        assert_eq!(delivered2.text, "second");

        // No more pending
        assert!(store.deliver("w1").is_none());
    }

    #[test]
    fn deliver_empty_returns_none() {
        let mut store = MessageStore::new();
        assert!(store.deliver("w1").is_none());
    }

    #[test]
    fn all_pending() {
        let mut store = MessageStore::new();
        store.enqueue(make_msg("pm", "w1", "a"));
        store.enqueue(make_msg("pm", "w2", "b"));
        assert_eq!(store.all_pending().len(), 2);

        store.deliver("w1");
        assert_eq!(store.all_pending().len(), 1);
        assert_eq!(store.all_pending()[0].recipient, "w2");
    }

    #[test]
    fn pending_excludes_delivered() {
        let mut store = MessageStore::new();
        store.enqueue(make_msg("pm", "w1", "x"));
        store.deliver("w1");
        assert_eq!(store.pending_for("w1").len(), 0);
    }

    #[test]
    fn multiple_recipients_independent() {
        let mut store = MessageStore::new();
        store.enqueue(make_msg("pm", "w1", "for-w1"));
        store.enqueue(make_msg("pm", "w2", "for-w2"));
        store.deliver("w1");
        // w2 still has pending
        assert_eq!(store.pending_for("w2").len(), 1);
    }

    #[test]
    fn len_counts_all() {
        let mut store = MessageStore::new();
        store.enqueue(make_msg("pm", "w1", "a"));
        store.enqueue(make_msg("pm", "w1", "b"));
        store.deliver("w1");
        assert_eq!(store.len(), 2); // both counted
    }

    #[test]
    fn deliver_marks_timestamp() {
        let mut store = MessageStore::new();
        store.enqueue(make_msg("pm", "w1", "test"));
        let msg = store.deliver("w1").unwrap();
        assert!(msg.delivered_at_ms.unwrap() > 0);
    }
}
