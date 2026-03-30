use serde::{Deserialize, Serialize};

use crate::event::PushEvent;

/// Directory name for the ATN coordination root.
pub const ATN_DIR_NAME: &str = ".atn";

/// Subdirectory for agent inboxes within the ATN root.
pub const INBOXES_DIR: &str = "inboxes";

/// Subdirectory for agent outboxes within the ATN root.
pub const OUTBOXES_DIR: &str = "outboxes";

/// A message in an agent's inbox or outbox.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InboxMessage {
    pub event: PushEvent,
    pub delivered: bool,
    pub delivered_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Priority, PushKind};

    #[test]
    fn inbox_message_roundtrip() {
        let msg = InboxMessage {
            event: PushEvent {
                id: "msg-001".to_string(),
                kind: PushKind::CompletionNotice,
                source_agent: "backend-dev".to_string(),
                source_repo: "backend".to_string(),
                target_agent: Some("frontend-dev".to_string()),
                issue_id: None,
                summary: "API endpoint ready".to_string(),
                wiki_link: None,
                priority: Priority::Normal,
                timestamp: "2026-03-29T15:00:00Z".to_string(),
            },
            delivered: false,
            delivered_at: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: InboxMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.event.id, msg.event.id);
        assert!(!back.delivered);
    }
}
