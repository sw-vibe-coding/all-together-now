use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use atn_core::error::{AtnError, Result};

const AGENTRAIL_DIR: &str = ".agentrail";

/// Saga configuration as stored in `.agentrail/saga.toml`.
#[derive(Debug, Deserialize, Serialize)]
pub struct SagaConfig {
    pub name: String,
    pub status: String,
    pub current_step: u32,
    pub created_at: String,
    pub plan_file: String,
}

/// Step configuration as stored in `.agentrail/steps/NNN-slug/step.toml`.
#[derive(Debug, Deserialize, Serialize)]
pub struct StepConfig {
    pub number: u32,
    pub slug: String,
    pub status: String,
    pub description: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub context_files: Vec<String>,
    #[serde(default)]
    pub task_type: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub completed_at: Option<String>,
}

/// ICRL trajectory record.
#[derive(Debug, Deserialize, Serialize)]
pub struct Trajectory {
    pub task_type: String,
    pub state: serde_json::Value,
    pub action: String,
    pub result: String,
    pub reward: i8,
    pub timestamp: String,
}

fn agentrail_dir(repo_path: &Path) -> PathBuf {
    repo_path.join(AGENTRAIL_DIR)
}

/// Load the saga configuration from a repo's `.agentrail/saga.toml`.
/// Returns `None` if no saga exists.
pub fn load_saga(repo_path: &Path) -> Result<Option<SagaConfig>> {
    let path = agentrail_dir(repo_path).join("saga.toml");
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let config: SagaConfig = toml::from_str(&content).map_err(AtnError::TomlDeser)?;
    Ok(Some(config))
}

/// List all steps in the saga, sorted by step number.
pub fn list_steps(repo_path: &Path) -> Result<Vec<StepConfig>> {
    let steps_dir = agentrail_dir(repo_path).join("steps");
    if !steps_dir.exists() {
        return Ok(Vec::new());
    }

    let mut steps = Vec::new();
    for entry in std::fs::read_dir(&steps_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let step_toml = entry.path().join("step.toml");
            if step_toml.exists() {
                let content = std::fs::read_to_string(&step_toml)?;
                if let Ok(step) = toml::from_str::<StepConfig>(&content) {
                    steps.push(step);
                }
            }
        }
    }
    steps.sort_by_key(|s| s.number);
    Ok(steps)
}

/// Load trajectories for a given task type.
pub fn load_trajectories(repo_path: &Path, task_type: &str) -> Result<Vec<Trajectory>> {
    let traj_dir = agentrail_dir(repo_path)
        .join("trajectories")
        .join(task_type);
    if !traj_dir.exists() {
        return Ok(Vec::new());
    }

    let mut trajectories = Vec::new();
    for entry in std::fs::read_dir(&traj_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            let content = std::fs::read_to_string(&path)?;
            if let Ok(traj) = serde_json::from_str::<Trajectory>(&content) {
                trajectories.push(traj);
            }
        }
    }
    trajectories.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    Ok(trajectories)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_saga_missing() {
        let dir = tempfile::tempdir().unwrap();
        let result = load_saga(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn load_saga_present() {
        let dir = tempfile::tempdir().unwrap();
        let at_dir = dir.path().join(AGENTRAIL_DIR);
        std::fs::create_dir_all(&at_dir).unwrap();
        std::fs::write(
            at_dir.join("saga.toml"),
            r#"
name = "test-saga"
status = "active"
current_step = 1
created_at = "2026-03-29T14:00:00Z"
plan_file = "plan.md"
"#,
        )
        .unwrap();

        let saga = load_saga(dir.path()).unwrap().unwrap();
        assert_eq!(saga.name, "test-saga");
        assert_eq!(saga.status, "active");
        assert_eq!(saga.current_step, 1);
    }

    #[test]
    fn list_steps_empty() {
        let dir = tempfile::tempdir().unwrap();
        let steps = list_steps(dir.path()).unwrap();
        assert!(steps.is_empty());
    }

    #[test]
    fn list_steps_ordered() {
        let dir = tempfile::tempdir().unwrap();
        let steps_dir = dir.path().join(AGENTRAIL_DIR).join("steps");

        for (num, slug) in [(2, "second"), (1, "first")] {
            let step_dir = steps_dir.join(format!("{num:03}-{slug}"));
            std::fs::create_dir_all(&step_dir).unwrap();
            std::fs::write(
                step_dir.join("step.toml"),
                format!(
                    r#"
number = {num}
slug = "{slug}"
status = "pending"
description = "Step {slug}"
role = "production"
created_at = "2026-03-29T14:00:00Z"
"#
                ),
            )
            .unwrap();
        }

        let steps = list_steps(dir.path()).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].slug, "first");
        assert_eq!(steps[1].slug, "second");
    }
}
