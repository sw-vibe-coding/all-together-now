use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent::{AgentConfig, AgentId, AgentRole};
use crate::error::Result;
use crate::spawn_spec::SpawnSpec;

/// Top-level agents.toml configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectConfig {
    #[serde(default)]
    pub project: ProjectMeta,
    #[serde(rename = "agent", default)]
    pub agents: Vec<AgentEntry>,
}

/// Project-level metadata.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProjectMeta {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub log_dir: Option<PathBuf>,
}

/// Single agent entry from agents.toml.
///
/// Two shapes are supported, mixable per-entry:
///
/// - **Flat** (legacy): `id`, `name`, `repo_path`, optional `role`,
///   `setup_commands`, and `launch_command` — a free-form shell line typed
///   into the outer bash.
/// - **Structured**: same id/name/repo_path plus an `[agent.spec]` subtable
///   holding a `SpawnSpec`. When present, `launch_command` is derived from
///   `spec.compose_command()` and the spec is registered in the server's
///   runtime `agent_specs` map so the UI can show/edit structured fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentEntry {
    pub id: String,
    pub name: String,
    pub repo_path: String,
    #[serde(default = "default_role")]
    pub role: AgentRole,
    #[serde(default)]
    pub setup_commands: Vec<String>,
    #[serde(default = "default_launch")]
    pub launch_command: String,
    /// Structured spawn specification. When set, takes precedence over the
    /// flat `launch_command`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<SpawnSpec>,
}

fn default_role() -> AgentRole {
    AgentRole::Developer
}

fn default_launch() -> String {
    String::new()
}

impl AgentEntry {
    /// Convert to the runtime AgentConfig, resolving relative repo_path against base_dir.
    ///
    /// If a structured `spec` is present, its composed shell command takes
    /// precedence over any literal `launch_command` — the UI dialog is the
    /// source of truth when both are supplied.
    pub fn to_agent_config(&self, base_dir: &Path) -> AgentConfig {
        let repo_path = if Path::new(&self.repo_path).is_absolute() {
            PathBuf::from(&self.repo_path)
        } else {
            base_dir.join(&self.repo_path)
        };
        let launch_command = match &self.spec {
            Some(spec) => spec.compose_command(),
            None => self.launch_command.clone(),
        };
        AgentConfig {
            id: AgentId(self.id.clone()),
            name: self.name.clone(),
            repo_path,
            role: self.role.clone(),
            setup_commands: self.setup_commands.clone(),
            launch_command,
        }
    }
}

/// Load project config from an agents.toml file.
pub fn load_project_config(path: &Path) -> Result<ProjectConfig> {
    let content = std::fs::read_to_string(path).map_err(crate::error::AtnError::Io)?;
    let config: ProjectConfig = toml::from_str(&content)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_agents_toml() {
        let toml_str = r#"
[project]
name = "my-project"
log_dir = "/tmp/atn-logs"

[[agent]]
id = "frontend"
name = "Frontend Dev"
repo_path = "/home/user/frontend"
role = "developer"
setup_commands = ["nvm use 18"]
launch_command = "claude"

[[agent]]
id = "backend"
name = "Backend Dev"
repo_path = "../backend"
role = "developer"
launch_command = "claude"

[[agent]]
id = "qa"
name = "QA Tester"
repo_path = "/home/user/tests"
role = "qa"
"#;
        let config: ProjectConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.project.name, "my-project");
        assert_eq!(config.agents.len(), 3);
        assert_eq!(config.agents[0].id, "frontend");
        assert_eq!(config.agents[1].role, AgentRole::Developer);
        assert_eq!(config.agents[2].role, AgentRole::QA);
    }

    #[test]
    fn parse_empty_agents_toml() {
        let toml_str = r#"
[project]
name = "empty-start"
"#;
        let config: ProjectConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.project.name, "empty-start");
        assert!(config.agents.is_empty());
    }

    #[test]
    fn parse_agents_toml_no_project_section() {
        let config: ProjectConfig = toml::from_str("").unwrap();
        assert!(config.agents.is_empty());
    }

    #[test]
    fn agent_entry_to_config_relative_path() {
        let entry = AgentEntry {
            id: "test".to_string(),
            name: "Test".to_string(),
            repo_path: "../other-repo".to_string(),
            role: AgentRole::Developer,
            setup_commands: vec![],
            launch_command: String::new(),
            spec: None,
        };
        let config = entry.to_agent_config(Path::new("/home/user/project"));
        assert_eq!(
            config.repo_path,
            PathBuf::from("/home/user/project/../other-repo")
        );
    }

    #[test]
    fn agent_entry_to_config_absolute_path() {
        let entry = AgentEntry {
            id: "test".to_string(),
            name: "Test".to_string(),
            repo_path: "/absolute/path".to_string(),
            role: AgentRole::Developer,
            setup_commands: vec![],
            launch_command: String::new(),
            spec: None,
        };
        let config = entry.to_agent_config(Path::new("/home/user/project"));
        assert_eq!(config.repo_path, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn spec_roundtrips_through_toml() {
        use crate::spawn_spec::{SpawnSpec, Transport};

        let toml_str = r#"
[project]
name = "roundtrip"

[[agent]]
id = "worker-hlasm"
name = "worker-hlasm (hlasm)"
repo_path = "."
role = "developer"
launch_command = ""

[agent.spec]
name = "worker-hlasm"
role = "worker"
transport = "mosh"
host = "queenbee"
user = "devh1"
working_dir = "/home/devh1/work/hlasm"
project = "hlasm"
agent = "codex"
"#;
        let parsed: ProjectConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.agents.len(), 1);
        let spec = parsed.agents[0].spec.as_ref().expect("spec round-trips");
        assert_eq!(spec.transport, Transport::Mosh);
        assert_eq!(spec.host.as_deref(), Some("queenbee"));
        assert_eq!(spec.user.as_deref(), Some("devh1"));

        // to_agent_config composes from spec, overriding the blank launch_command.
        let config = parsed.agents[0].to_agent_config(Path::new("/base"));
        assert_eq!(
            config.launch_command,
            "mosh devh1@queenbee -- tmux new-session -A -s atn-worker-hlasm 'cd /home/devh1/work/hlasm && codex'"
        );

        // Serializing back produces a config that's load-equivalent.
        let reserialized = toml::to_string(&parsed).unwrap();
        let reparsed: ProjectConfig = toml::from_str(&reserialized).unwrap();
        assert_eq!(
            reparsed.agents[0].spec.as_ref().unwrap(),
            &SpawnSpec {
                name: "worker-hlasm".to_string(),
                role: "worker".to_string(),
                transport: Transport::Mosh,
                host: Some("queenbee".to_string()),
                user: Some("devh1".to_string()),
                working_dir: "/home/devh1/work/hlasm".to_string(),
                project: Some("hlasm".to_string()),
                agent: "codex".to_string(),
                agent_args: None,
            }
        );
    }

    #[test]
    fn entries_without_spec_omit_the_field_when_serialized() {
        let entry = AgentEntry {
            id: "legacy".to_string(),
            name: "Legacy".to_string(),
            repo_path: ".".to_string(),
            role: AgentRole::Developer,
            setup_commands: vec![],
            launch_command: "bash".to_string(),
            spec: None,
        };
        let toml_str = toml::to_string(&entry).unwrap();
        assert!(
            !toml_str.contains("spec"),
            "expected no [spec] subtable for legacy entry; got:\n{toml_str}"
        );
    }
}
