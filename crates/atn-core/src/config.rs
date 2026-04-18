use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent::{AgentConfig, AgentId, AgentRole};
use crate::error::Result;

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
}

fn default_role() -> AgentRole {
    AgentRole::Developer
}

fn default_launch() -> String {
    String::new()
}

impl AgentEntry {
    /// Convert to the runtime AgentConfig, resolving relative repo_path against base_dir.
    pub fn to_agent_config(&self, base_dir: &Path) -> AgentConfig {
        let repo_path = if Path::new(&self.repo_path).is_absolute() {
            PathBuf::from(&self.repo_path)
        } else {
            base_dir.join(&self.repo_path)
        };
        AgentConfig {
            id: AgentId(self.id.clone()),
            name: self.name.clone(),
            repo_path,
            role: self.role.clone(),
            setup_commands: self.setup_commands.clone(),
            launch_command: self.launch_command.clone(),
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
        };
        let config = entry.to_agent_config(Path::new("/home/user/project"));
        assert_eq!(config.repo_path, PathBuf::from("/absolute/path"));
    }
}
