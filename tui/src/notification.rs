//! Notification center — manages transient and persistent notifications.
//!
//! `NotificationCenter` is a bounded queue of `Notification` items. Each
//! notification has a type, a body, an optional source, and a timestamp.
//! Expired notifications are pruned on access. The center can hold up to
//! a configurable maximum number of entries.

use serde::{Deserialize, Serialize};


// ---------------------------------------------------------------------------
// NotificationType
// ---------------------------------------------------------------------------

/// The severity / category of a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationType {
    /// Informational, no action needed.
    Info,
    /// Something noteworthy — review when convenient.
    Warning,
    /// A problem that may need attention.
    Error,
    /// Something positive — e.g. a task completed.
    Success,
    /// An agent or task status change.
    StatusChange,
    /// A system event (daemon start, shutdown, etc.).
    System,
}

impl NotificationType {
    /// Return a short label suitable for display.
    pub fn label(&self) -> &str {
        match self {
            NotificationType::Info => "info",
            NotificationType::Warning => "warn",
            NotificationType::Error => "error",
            NotificationType::Success => "ok",
            NotificationType::StatusChange => "status",
            NotificationType::System => "system",
        }
    }

    /// Return the ANSI color code for this type.
    pub fn color(&self) -> &str {
        match self {
            NotificationType::Info => "\x1b[36m",     // cyan
            NotificationType::Warning => "\x1b[33m",  // yellow
            NotificationType::Error => "\x1b[31m",    // red
            NotificationType::Success => "\x1b[32m",  // green
            NotificationType::StatusChange => "\x1b[34m", // blue
            NotificationType::System => "\x1b[37m",   // white
        }
    }
}


// ---------------------------------------------------------------------------
// Notification
// ---------------------------------------------------------------------------

/// A single notification entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Unique ID for this notification.
    pub id: u64,
    /// The type / severity of the notification.
    pub notification_type: NotificationType,
    /// The main body text.
    pub body: String,
    /// Optional source of the notification (e.g. agent name, task ID).
    pub source: Option<String>,
    /// Timestamp (ms since epoch) when the notification was created.
    pub created_ms: u64,
    /// Whether this notification has been read / acknowledged.
    pub read: bool,
    /// Time-to-live in milliseconds. `None` means the notification persists
    /// until explicitly dismissed.
    pub ttl_ms: Option<u64>,
}

impl Notification {
    /// Create a new unread notification.
    pub fn new(
        id: u64,
        notification_type: NotificationType,
        body: &str,
        source: Option<&str>,
        created_ms: u64,
        ttl_ms: Option<u64>,
    ) -> Self {
        Notification {
            id,
            notification_type,
            body: body.to_string(),
            source: source.map(|s| s.to_string()),
            created_ms,
            read: false,
            ttl_ms,
        }
    }

    /// Whether this notification has expired at the given time.
    pub fn is_expired(&self, now_ms: u64) -> bool {
        if let Some(ttl) = self.ttl_ms {
            now_ms.saturating_sub(self.created_ms) >= ttl
        } else {
            false
        }
    }

    /// Mark this notification as read.
    pub fn mark_read(&mut self) {
        self.read = true;
    }

    /// Return a formatted one-line summary.
    pub fn summary(&self) -> String {
        let source_part = match &self.source {
            Some(s) => format!(" [{}]", s),
            None => String::new(),
        };
        format!(
            "[{}] {}{}",
            self.notification_type.label(),
            self.body,
            source_part,
        )
    }

    /// Return a colored one-line summary with ANSI codes.
    pub fn colored_summary(&self) -> String {
        let color = self.notification_type.color();
        let reset = "\x1b[0m";
        let source_part = match &self.source {
            Some(s) => format!(" [{}]", s),
            None => String::new(),
        };
        format!(
            "{}[{}]{} {}{}",
            color,
            self.notification_type.label(),
            reset,
            self.body,
            source_part,
        )
    }
}


// ---------------------------------------------------------------------------
// NotificationCenter
// ---------------------------------------------------------------------------

/// Bounded notification queue with TTL-based expiry.
pub struct NotificationCenter {
    notifications: Vec<Notification>,
    max_entries: usize,
    next_id: u64,
}

impl NotificationCenter {
    /// Create a new notification center with the given capacity.
    pub fn new(max_entries: usize) -> Self {
        NotificationCenter {
            notifications: Vec::new(),
            max_entries,
            next_id: 1,
        }
    }

    /// Add a notification with auto-assigned ID. Returns the assigned ID.
    pub fn push(
        &mut self,
        notification_type: NotificationType,
        body: &str,
        source: Option<&str>,
        now_ms: u64,
        ttl_ms: Option<u64>,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let notification = Notification::new(id, notification_type, body, source, now_ms, ttl_ms);
        self.notifications.push(notification);

        // Enforce capacity limit (remove oldest first).
        while self.notifications.len() > self.max_entries {
            self.notifications.remove(0);
        }

        id
    }

    /// Add a pre-built notification. The notification's ID is used as-is.
    pub fn push_notification(&mut self, notification: Notification) {
        if notification.id >= self.next_id {
            self.next_id = notification.id + 1;
        }
        self.notifications.push(notification);

        while self.notifications.len() > self.max_entries {
            self.notifications.remove(0);
        }
    }

    /// Prune expired notifications relative to `now_ms`.
    pub fn prune(&mut self, now_ms: u64) {
        self.notifications
            .retain(|n| !n.is_expired(now_ms));
    }

    /// Remove a notification by ID. Returns true if found and removed.
    pub fn dismiss(&mut self, id: u64) -> bool {
        let before = self.notifications.len();
        self.notifications.retain(|n| n.id != id);
        self.notifications.len() < before
    }

    /// Mark a notification as read by ID. Returns true if found.
    pub fn mark_read(&mut self, id: u64) -> bool {
        if let Some(n) = self.notifications.iter_mut().find(|n| n.id == id) {
            n.mark_read();
            true
        } else {
            false
        }
    }

    /// Mark all notifications as read.
    pub fn mark_all_read(&mut self) {
        for n in &mut self.notifications {
            n.mark_read();
        }
    }

    /// Dismiss all notifications. Returns the number removed.
    pub fn clear(&mut self) -> usize {
        let count = self.notifications.len();
        self.notifications.clear();
        count
    }

    /// Return the total number of notifications.
    pub fn len(&self) -> usize {
        self.notifications.len()
    }

    /// Return true if there are no notifications.
    pub fn is_empty(&self) -> bool {
        self.notifications.is_empty()
    }

    /// Return the count of unread notifications.
    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.read).count()
    }

    /// Return all notifications (newest last).
    pub fn all(&self) -> &[Notification] {
        &self.notifications
    }

    /// Return the most recent `n` notifications.
    pub fn recent(&self, n: usize) -> &[Notification] {
        let start = self.notifications.len().saturating_sub(n);
        &self.notifications[start..]
    }

    /// Return all unread notifications.
    pub fn unread(&self) -> Vec<&Notification> {
        self.notifications.iter().filter(|n| !n.read).collect()
    }

    /// Return all notifications of a given type.
    pub fn by_type(&self, notification_type: NotificationType) -> Vec<&Notification> {
        self.notifications
            .iter()
            .filter(|n| n.notification_type == notification_type)
            .collect()
    }

    /// Return all notifications from a given source.
    pub fn by_source(&self, source: &str) -> Vec<&Notification> {
        self.notifications
            .iter()
            .filter(|n| n.source.as_deref() == Some(source))
            .collect()
    }

    /// Return a notification by ID.
    pub fn get(&self, id: u64) -> Option<&Notification> {
        self.notifications.iter().find(|n| n.id == id)
    }

    /// Return the latest notification, if any.
    pub fn latest(&self) -> Option<&Notification> {
        self.notifications.last()
    }

    /// Return the latest unread notification, if any.
    pub fn latest_unread(&self) -> Option<&Notification> {
        self.notifications.iter().rev().find(|n| !n.read)
    }
}


impl Default for NotificationCenter {
    fn default() -> Self {
        NotificationCenter::new(100)
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- NotificationType ---

    #[test]
    fn notification_type_labels() {
        assert_eq!(NotificationType::Info.label(), "info");
        assert_eq!(NotificationType::Warning.label(), "warn");
        assert_eq!(NotificationType::Error.label(), "error");
        assert_eq!(NotificationType::Success.label(), "ok");
        assert_eq!(NotificationType::StatusChange.label(), "status");
        assert_eq!(NotificationType::System.label(), "system");
    }

    #[test]
    fn notification_type_colors() {
        // Each type should return a non-empty ANSI escape.
        assert!(!NotificationType::Info.color().is_empty());
        assert!(!NotificationType::Warning.color().is_empty());
        assert!(!NotificationType::Error.color().is_empty());
        assert!(!NotificationType::Success.color().is_empty());
        assert!(!NotificationType::StatusChange.color().is_empty());
        assert!(!NotificationType::System.color().is_empty());
    }

    #[test]
    fn notification_type_serde_round_trip() {
        let types = [
            NotificationType::Info,
            NotificationType::Warning,
            NotificationType::Error,
            NotificationType::Success,
            NotificationType::StatusChange,
            NotificationType::System,
        ];
        for t in &types {
            let json = serde_json::to_string(t).unwrap();
            let back: NotificationType = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, t);
        }
    }

    // --- Notification ---

    #[test]
    fn notification_new_fields() {
        let n = Notification::new(1, NotificationType::Info, "hello", Some("w1"), 1000, None);
        assert_eq!(n.id, 1);
        assert_eq!(n.notification_type, NotificationType::Info);
        assert_eq!(n.body, "hello");
        assert_eq!(n.source, Some("w1".into()));
        assert_eq!(n.created_ms, 1000);
        assert!(!n.read);
        assert!(n.ttl_ms.is_none());
    }

    #[test]
    fn notification_expiry_with_ttl() {
        let n = Notification::new(1, NotificationType::Info, "msg", None, 1000, Some(5000));
        assert!(!n.is_expired(3000));  // 2000ms later, ttl=5000
        assert!(n.is_expired(6000));   // 5000ms later
        assert!(n.is_expired(7000));   // past deadline
    }

    #[test]
    fn notification_expiry_exact_boundary() {
        let n = Notification::new(1, NotificationType::Info, "msg", None, 1000, Some(5000));
        assert!(n.is_expired(6000)); // exactly at deadline
    }

    #[test]
    fn notification_no_ttl_never_expires() {
        let n = Notification::new(1, NotificationType::Info, "msg", None, 1000, None);
        assert!(!n.is_expired(9999999));
    }

    #[test]
    fn notification_mark_read() {
        let mut n = Notification::new(1, NotificationType::Info, "msg", None, 1000, None);
        assert!(!n.read);
        n.mark_read();
        assert!(n.read);
    }

    #[test]
    fn notification_summary() {
        let n = Notification::new(1, NotificationType::Warning, "disk full", Some("host1"), 1000, None);
        let s = n.summary();
        assert!(s.contains("[warn]"));
        assert!(s.contains("disk full"));
        assert!(s.contains("[host1]"));
    }

    #[test]
    fn notification_summary_no_source() {
        let n = Notification::new(1, NotificationType::Error, "crash", None, 1000, None);
        let s = n.summary();
        assert!(s.contains("[error]"));
        assert!(s.contains("crash"));
        assert!(!s.contains("[]"));
    }

    #[test]
    fn notification_colored_summary() {
        let n = Notification::new(1, NotificationType::Success, "done", None, 1000, None);
        let s = n.colored_summary();
        assert!(s.contains("[ok]"));
        assert!(s.contains("done"));
        assert!(s.contains("\x1b["));  // contains ANSI escape
    }

    #[test]
    fn notification_serde_round_trip() {
        let n = Notification::new(42, NotificationType::Error, "boom", Some("w1"), 5000, Some(10000));
        let json = serde_json::to_string(&n).unwrap();
        let back: Notification = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, 42);
        assert_eq!(back.body, "boom");
        assert_eq!(back.notification_type, NotificationType::Error);
        assert_eq!(back.source, Some("w1".into()));
        assert_eq!(back.ttl_ms, Some(10000));
    }

    // --- NotificationCenter ---

    #[test]
    fn center_new_is_empty() {
        let nc = NotificationCenter::new(10);
        assert!(nc.is_empty());
        assert_eq!(nc.len(), 0);
        assert_eq!(nc.unread_count(), 0);
    }

    #[test]
    fn center_default_capacity() {
        let nc = NotificationCenter::default();
        assert!(nc.is_empty());
        // Default capacity is 100 — push 101 items.
        // (Not testing here, just that default works.)
    }

    #[test]
    fn center_push_returns_id() {
        let mut nc = NotificationCenter::new(10);
        let id1 = nc.push(NotificationType::Info, "first", None, 1000, None);
        let id2 = nc.push(NotificationType::Info, "second", None, 2000, None);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn center_push_increments_count() {
        let mut nc = NotificationCenter::new(10);
        nc.push(NotificationType::Info, "a", None, 1000, None);
        nc.push(NotificationType::Warning, "b", None, 2000, None);
        assert_eq!(nc.len(), 2);
    }

    #[test]
    fn center_capacity_limit() {
        let mut nc = NotificationCenter::new(3);
        nc.push(NotificationType::Info, "a", None, 1000, None);
        nc.push(NotificationType::Info, "b", None, 2000, None);
        nc.push(NotificationType::Info, "c", None, 3000, None);
        nc.push(NotificationType::Info, "d", None, 4000, None);

        assert_eq!(nc.len(), 3);
        // Oldest ("a") should have been removed.
        let bodies: Vec<&str> = nc.all().iter().map(|n| n.body.as_str()).collect();
        assert!(!bodies.contains(&"a"));
        assert!(bodies.contains(&"d"));
    }

    #[test]
    fn center_push_notification() {
        let mut nc = NotificationCenter::new(10);
        let n = Notification::new(99, NotificationType::Error, "test", None, 1000, None);
        nc.push_notification(n);
        assert_eq!(nc.len(), 1);
        assert_eq!(nc.get(99).unwrap().body, "test");
    }

    #[test]
    fn center_push_notification_updates_next_id() {
        let mut nc = NotificationCenter::new(10);
        let n = Notification::new(50, NotificationType::Info, "test", None, 1000, None);
        nc.push_notification(n);
        let id = nc.push(NotificationType::Info, "next", None, 2000, None);
        assert!(id > 50);
    }

    #[test]
    fn center_prune_removes_expired() {
        let mut nc = NotificationCenter::new(10);
        nc.push(NotificationType::Info, "short", None, 1000, Some(2000));
        nc.push(NotificationType::Info, "long", None, 1000, Some(10000));
        nc.push(NotificationType::Info, "forever", None, 1000, None);

        nc.prune(5000); // 4000ms later: "short" expired (ttl=2000), others alive
        assert_eq!(nc.len(), 2);
    }

    #[test]
    fn center_prune_keeps_unexpired() {
        let mut nc = NotificationCenter::new(10);
        nc.push(NotificationType::Info, "a", None, 1000, Some(10000));
        nc.prune(2000); // only 1000ms, ttl=10000
        assert_eq!(nc.len(), 1);
    }

    #[test]
    fn center_dismiss_by_id() {
        let mut nc = NotificationCenter::new(10);
        let id = nc.push(NotificationType::Info, "a", None, 1000, None);
        assert!(nc.dismiss(id));
        assert!(nc.is_empty());
    }

    #[test]
    fn center_dismiss_nonexistent() {
        let mut nc = NotificationCenter::new(10);
        assert!(!nc.dismiss(999));
    }

    #[test]
    fn center_mark_read() {
        let mut nc = NotificationCenter::new(10);
        let id = nc.push(NotificationType::Info, "a", None, 1000, None);
        assert_eq!(nc.unread_count(), 1);
        assert!(nc.mark_read(id));
        assert_eq!(nc.unread_count(), 0);
    }

    #[test]
    fn center_mark_read_nonexistent() {
        let mut nc = NotificationCenter::new(10);
        assert!(!nc.mark_read(999));
    }

    #[test]
    fn center_mark_all_read() {
        let mut nc = NotificationCenter::new(10);
        nc.push(NotificationType::Info, "a", None, 1000, None);
        nc.push(NotificationType::Warning, "b", None, 2000, None);
        nc.push(NotificationType::Error, "c", None, 3000, None);
        assert_eq!(nc.unread_count(), 3);
        nc.mark_all_read();
        assert_eq!(nc.unread_count(), 0);
    }

    #[test]
    fn center_clear() {
        let mut nc = NotificationCenter::new(10);
        nc.push(NotificationType::Info, "a", None, 1000, None);
        nc.push(NotificationType::Info, "b", None, 2000, None);
        let removed = nc.clear();
        assert_eq!(removed, 2);
        assert!(nc.is_empty());
    }

    #[test]
    fn center_recent() {
        let mut nc = NotificationCenter::new(10);
        nc.push(NotificationType::Info, "a", None, 1000, None);
        nc.push(NotificationType::Info, "b", None, 2000, None);
        nc.push(NotificationType::Info, "c", None, 3000, None);

        let recent = nc.recent(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].body, "b");
        assert_eq!(recent[1].body, "c");
    }

    #[test]
    fn center_recent_more_than_available() {
        let mut nc = NotificationCenter::new(10);
        nc.push(NotificationType::Info, "a", None, 1000, None);
        let recent = nc.recent(5);
        assert_eq!(recent.len(), 1);
    }

    #[test]
    fn center_unread() {
        let mut nc = NotificationCenter::new(10);
        let id1 = nc.push(NotificationType::Info, "a", None, 1000, None);
        nc.push(NotificationType::Info, "b", None, 2000, None);
        nc.mark_read(id1);

        let unread = nc.unread();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].body, "b");
    }

    #[test]
    fn center_by_type() {
        let mut nc = NotificationCenter::new(10);
        nc.push(NotificationType::Info, "info1", None, 1000, None);
        nc.push(NotificationType::Error, "err1", None, 2000, None);
        nc.push(NotificationType::Info, "info2", None, 3000, None);

        let infos = nc.by_type(NotificationType::Info);
        assert_eq!(infos.len(), 2);
        let errors = nc.by_type(NotificationType::Error);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn center_by_source() {
        let mut nc = NotificationCenter::new(10);
        nc.push(NotificationType::Info, "a", Some("w1"), 1000, None);
        nc.push(NotificationType::Info, "b", Some("w2"), 2000, None);
        nc.push(NotificationType::Info, "c", Some("w1"), 3000, None);

        let w1 = nc.by_source("w1");
        assert_eq!(w1.len(), 2);
        let w2 = nc.by_source("w2");
        assert_eq!(w2.len(), 1);
    }

    #[test]
    fn center_get_by_id() {
        let mut nc = NotificationCenter::new(10);
        let id = nc.push(NotificationType::Info, "target", None, 1000, None);
        let n = nc.get(id).unwrap();
        assert_eq!(n.body, "target");
    }

    #[test]
    fn center_get_nonexistent() {
        let nc = NotificationCenter::new(10);
        assert!(nc.get(999).is_none());
    }

    #[test]
    fn center_latest() {
        let mut nc = NotificationCenter::new(10);
        nc.push(NotificationType::Info, "first", None, 1000, None);
        nc.push(NotificationType::Info, "second", None, 2000, None);
        assert_eq!(nc.latest().unwrap().body, "second");
    }

    #[test]
    fn center_latest_empty() {
        let nc = NotificationCenter::new(10);
        assert!(nc.latest().is_none());
    }

    #[test]
    fn center_latest_unread() {
        let mut nc = NotificationCenter::new(10);
        let id1 = nc.push(NotificationType::Info, "first", None, 1000, None);
        let id2 = nc.push(NotificationType::Info, "second", None, 2000, None);
        nc.mark_read(id2);

        let latest_unread = nc.latest_unread().unwrap();
        assert_eq!(latest_unread.id, id1);
    }

    #[test]
    fn center_latest_unread_all_read() {
        let mut nc = NotificationCenter::new(10);
        let id = nc.push(NotificationType::Info, "only", None, 1000, None);
        nc.mark_read(id);
        assert!(nc.latest_unread().is_none());
    }
}
