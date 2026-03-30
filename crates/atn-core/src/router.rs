use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::event::PushEvent;

/// Outcome of routing a push event through the message router.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteDecision {
    /// Deliver to a known agent's PTY inbox.
    DeliverToAgent { agent_id: String },
    /// Target agent is unknown — escalate to wiki + broadcast notification.
    Escalate { reason: String },
    /// No target specified — broadcast to wiki Requests page.
    Broadcast,
}

impl fmt::Display for RouteDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RouteDecision::DeliverToAgent { agent_id } => write!(f, "deliver:{agent_id}"),
            RouteDecision::Escalate { reason } => write!(f, "escalate:{reason}"),
            RouteDecision::Broadcast => write!(f, "broadcast"),
        }
    }
}

/// An entry in the append-only event log.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventLogEntry {
    pub event: PushEvent,
    pub decision: String,
    pub delivered: bool,
    pub logged_at: String,
}

/// Decide how to route a push event given the set of known agent IDs.
pub fn route_event(event: &PushEvent, known_agents: &HashSet<String>) -> RouteDecision {
    match &event.target_agent {
        Some(target) if known_agents.contains(target) => RouteDecision::DeliverToAgent {
            agent_id: target.clone(),
        },
        Some(target) => RouteDecision::Escalate {
            reason: format!("unknown target agent: {target}"),
        },
        None => RouteDecision::Broadcast,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Priority, PushKind};

    fn test_event(target: Option<&str>) -> PushEvent {
        PushEvent {
            id: "evt-test".to_string(),
            kind: PushKind::FeatureRequest,
            source_agent: "alice".to_string(),
            source_repo: ".".to_string(),
            target_agent: target.map(|s| s.to_string()),
            issue_id: None,
            summary: "test event".to_string(),
            wiki_link: None,
            priority: Priority::Normal,
            timestamp: "2026-03-29T18:00:00Z".to_string(),
        }
    }

    #[test]
    fn route_to_known_agent() {
        let known: HashSet<String> = ["alice", "bob"].iter().map(|s| s.to_string()).collect();
        let event = test_event(Some("bob"));
        assert_eq!(
            route_event(&event, &known),
            RouteDecision::DeliverToAgent {
                agent_id: "bob".to_string()
            }
        );
    }

    #[test]
    fn route_to_unknown_agent_escalates() {
        let known: HashSet<String> = ["alice"].iter().map(|s| s.to_string()).collect();
        let event = test_event(Some("charlie"));
        match route_event(&event, &known) {
            RouteDecision::Escalate { reason } => {
                assert!(reason.contains("charlie"));
            }
            other => panic!("expected Escalate, got {other:?}"),
        }
    }

    #[test]
    fn route_no_target_broadcasts() {
        let known: HashSet<String> = ["alice"].iter().map(|s| s.to_string()).collect();
        let event = test_event(None);
        assert_eq!(route_event(&event, &known), RouteDecision::Broadcast);
    }
}
