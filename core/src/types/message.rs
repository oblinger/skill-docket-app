use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub sender: String,
    pub recipient: String,
    pub text: String,
    pub queued_at_ms: u64,
    pub delivered_at_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_round_trip() {
        let msg = Message {
            sender: "pm".into(),
            recipient: "worker-1".into(),
            text: "start task CMX1".into(),
            queued_at_ms: 1700000000000,
            delivered_at_ms: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn message_with_delivery() {
        let msg = Message {
            sender: "worker-1".into(),
            recipient: "pm".into(),
            text: "task complete".into(),
            queued_at_ms: 1700000000000,
            delivered_at_ms: Some(1700000001000),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"delivered_at_ms\":1700000001000"));
    }
}
