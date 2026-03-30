use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

use crate::event::PushEvent;

/// Unique identifier for an agent session.
#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct AgentId(pub String);

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for AgentId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Role an agent plays in the coordination workflow.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentRole {
    Developer,
    QA,
    PM,
    Coordinator,
}

/// Configuration for spawning an agent session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: AgentId,
    pub name: String,
    pub repo_path: PathBuf,
    pub role: AgentRole,
    pub setup_commands: Vec<String>,
    pub launch_command: String,
}

/// Current state of an agent session.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AgentState {
    Starting,
    Running,
    AwaitingHumanInput,
    Busy,
    Blocked { on: Vec<String> },
    Idle,
    CompletedTask,
    Error { message: String },
    Disconnected,
}

/// Full status snapshot for an agent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentStatus {
    pub config: AgentConfig,
    pub state: AgentState,
    pub last_output_at: Option<String>,
    pub pending_requests: Vec<PushEvent>,
    pub current_task: Option<String>,
    pub saga_step: Option<(u32, String)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_id_display() {
        let id = AgentId("frontend-dev".to_string());
        assert_eq!(id.to_string(), "frontend-dev");
    }

    #[test]
    fn agent_config_roundtrip() {
        let config = AgentConfig {
            id: AgentId("test".to_string()),
            name: "Test Agent".to_string(),
            repo_path: PathBuf::from("/tmp/test"),
            role: AgentRole::Developer,
            setup_commands: vec!["cd /tmp".to_string()],
            launch_command: "claude".to_string(),
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: AgentConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, config.id);
        assert_eq!(back.name, config.name);
    }

    #[test]
    fn agent_state_roundtrip() {
        let states = vec![
            AgentState::Starting,
            AgentState::Running,
            AgentState::AwaitingHumanInput,
            AgentState::Busy,
            AgentState::Blocked {
                on: vec!["dep-a".to_string()],
            },
            AgentState::Idle,
            AgentState::CompletedTask,
            AgentState::Error {
                message: "oops".to_string(),
            },
            AgentState::Disconnected,
        ];
        for state in states {
            let json = serde_json::to_string(&state).unwrap();
            let back: AgentState = serde_json::from_str(&json).unwrap();
            assert_eq!(back, state);
        }
    }
}
