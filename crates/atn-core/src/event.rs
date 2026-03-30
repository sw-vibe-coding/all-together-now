use serde::{Deserialize, Serialize};

/// Input sent to an agent's PTY.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputEvent {
    HumanText { text: String },
    RawBytes { bytes: Vec<u8> },
    CoordinatorCommand { command: String },
    Action { action: CannedAction },
}

/// Pre-defined actions with known byte sequences or workflows.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CannedAction {
    CtrlC,
    ClaudeGo,
    ReadWiki { page: String },
    Ack { request_id: String },
}

/// Signal emitted from the PTY output parser.
#[derive(Clone, Debug)]
pub enum OutputSignal {
    Bytes(Vec<u8>),
    PromptReady,
    QuestionDetected { snippet: String },
    IdleDetected,
    PushEvent(PushEvent),
}

/// A structured inter-agent push event.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PushEvent {
    pub id: String,
    pub kind: PushKind,
    pub source_agent: String,
    pub source_repo: String,
    pub target_agent: Option<String>,
    pub issue_id: Option<String>,
    pub summary: String,
    pub wiki_link: Option<String>,
    pub priority: Priority,
    pub timestamp: String,
}

/// Type of inter-agent push event.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PushKind {
    FeatureRequest,
    BugFixRequest,
    CompletionNotice,
    BlockedNotice,
    NeedsInfo,
    VerificationRequest,
}

/// Priority level for a push event.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Normal,
    High,
    Blocking,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_event_roundtrip() {
        let event = PushEvent {
            id: "evt-001".to_string(),
            kind: PushKind::FeatureRequest,
            source_agent: "frontend-dev".to_string(),
            source_repo: "frontend".to_string(),
            target_agent: Some("backend-dev".to_string()),
            issue_id: Some("FEAT-42".to_string()),
            summary: "Need API endpoint for user profiles".to_string(),
            wiki_link: Some("Coordination/Requests".to_string()),
            priority: Priority::High,
            timestamp: "2026-03-29T14:00:00Z".to_string(),
        };
        let json = serde_json::to_string_pretty(&event).unwrap();
        let back: PushEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, event.id);
        assert_eq!(back.kind, event.kind);
        assert_eq!(back.priority, event.priority);
    }

    #[test]
    fn input_event_roundtrip() {
        let events: Vec<InputEvent> = vec![
            InputEvent::HumanText {
                text: "hello".to_string(),
            },
            InputEvent::RawBytes {
                bytes: vec![0x03],
            },
            InputEvent::CoordinatorCommand {
                command: "coord inbox".to_string(),
            },
            InputEvent::Action {
                action: CannedAction::CtrlC,
            },
            InputEvent::Action {
                action: CannedAction::ClaudeGo,
            },
        ];
        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            let _back: InputEvent = serde_json::from_str(&json).unwrap();
        }
    }
}
