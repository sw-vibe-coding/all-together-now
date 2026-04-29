//! Structured spawn specification for agents.
//!
//! A `SpawnSpec` captures the parts needed to start an agent — where to run
//! (local, mosh, ssh), which user@host for remote transports, the working
//! directory, and the agent CLI. From a `SpawnSpec` the server composes the
//! actual shell command that the PTY types into a login shell.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::watchdog::WatchdogConfig;

/// How the PTY reaches the agent process.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// Run the agent directly on this machine.
    Local,
    /// Connect via mosh (resilient to network drops).
    Mosh,
    /// Connect via plain ssh.
    Ssh,
}

impl Transport {
    pub fn is_remote(self) -> bool {
        !matches!(self, Transport::Local)
    }

    /// Shell binary name (`mosh` / `ssh`) for remote transports.
    pub fn binary(self) -> Option<&'static str> {
        match self {
            Transport::Local => None,
            Transport::Mosh => Some("mosh"),
            Transport::Ssh => Some("ssh"),
        }
    }
}

/// Full structured description of a new agent spawn.
///
/// Required fields: `name`, `working_dir`, `agent`. `host` and `user` are
/// required when `transport` is not `Local`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SpawnSpec {
    /// Unique agent id and display name seed.
    pub name: String,
    /// High-level role — coordinator, worker, or a custom string.
    #[serde(default = "default_role")]
    pub role: String,
    /// Local / mosh / ssh.
    #[serde(default = "default_transport")]
    pub transport: Transport,
    /// Remote host (required for mosh/ssh).
    #[serde(default)]
    pub host: Option<String>,
    /// Remote login user (required for mosh/ssh).
    #[serde(default)]
    pub user: Option<String>,
    /// Working directory on the target machine.
    pub working_dir: String,
    /// Optional label for the project; defaults to basename of working_dir.
    #[serde(default)]
    pub project: Option<String>,
    /// Agent CLI: claude / codex / opencode / gemini / custom binary name.
    pub agent: String,
    /// Optional free-form args appended to the agent command.
    #[serde(default)]
    pub agent_args: Option<String>,
    /// Optional initial-prompt positional argument. Compose wraps this
    /// in single quotes so multi-word prompts (e.g., "read AGENTS.md
    /// and follow it") survive the shell. Single quotes in the value
    /// are forbidden (validated against the same FORBIDDEN_CHARS list
    /// as the rest).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_prompt: Option<String>,
    /// Optional per-agent watchdog thresholds. Defaults apply when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watchdog: Option<WatchdogConfig>,
}

fn default_role() -> String {
    "worker".to_string()
}

fn default_transport() -> Transport {
    Transport::Local
}

/// Fields that must be present for the given transport.
pub const LOCAL_REQUIRED: &[&str] = &["name", "working_dir", "agent"];
pub const REMOTE_REQUIRED: &[&str] = &["name", "host", "user", "working_dir", "agent"];

/// Characters that are not allowed in shell-interpolated fields.
/// Keeps the initial surface conservative; expand as escape coverage grows.
const FORBIDDEN_CHARS: &[char] = &['\'', '"', '`', '\n', '\r', '\0', '\\'];

impl SpawnSpec {
    /// Returns the list of missing-or-invalid field names, or `Ok(())` if valid.
    pub fn validate(&self) -> Result<(), Vec<&'static str>> {
        let mut missing = Vec::new();

        if self.name.trim().is_empty() {
            missing.push("name");
        }
        if self.working_dir.trim().is_empty() {
            missing.push("working_dir");
        }
        if self.agent.trim().is_empty() {
            missing.push("agent");
        }

        if self.transport.is_remote() {
            if self.host.as_deref().map(str::trim).unwrap_or("").is_empty() {
                missing.push("host");
            }
            if self.user.as_deref().map(str::trim).unwrap_or("").is_empty() {
                missing.push("user");
            }
        }

        // Reject fields with shell-metacharacters we don't safely escape yet.
        for (label, value) in [
            ("name", Some(self.name.as_str())),
            ("host", self.host.as_deref()),
            ("user", self.user.as_deref()),
            ("working_dir", Some(self.working_dir.as_str())),
            ("agent", Some(self.agent.as_str())),
            ("agent_args", self.agent_args.as_deref()),
            ("agent_prompt", self.agent_prompt.as_deref()),
        ] {
            if let Some(v) = value
                && v.chars().any(|c| FORBIDDEN_CHARS.contains(&c))
            {
                missing.push(static_label(label));
            }
        }

        if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        }
    }

    /// Human-friendly project label — falls back to basename of `working_dir`.
    pub fn project_label(&self) -> String {
        if let Some(p) = self.project.as_ref() {
            let t = p.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
        Path::new(&self.working_dir)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| self.working_dir.clone())
    }

    /// The tmux session name used on the remote for mosh/ssh transports.
    pub fn tmux_session(&self) -> String {
        format!("atn-{}", self.name)
    }

    /// Compose the full shell command that the outer PTY shell will execute.
    ///
    /// For `Local`: `cd <dir> && <agent> [args]`
    ///
    /// For `Mosh`/`Ssh`: `<bin> <user>@<host> -- tmux new-session -A -s atn-<name> '<inner>'`
    ///                   where `<inner>` is `cd <dir> && <agent> [args]`.
    pub fn compose_command(&self) -> String {
        let agent_tail = match self.agent_args.as_ref().map(|s| s.trim()) {
            Some(args) if !args.is_empty() => format!("{} {}", self.agent, args),
            _ => self.agent.clone(),
        };
        let prompt_tail = match self.agent_prompt.as_ref().map(|s| s.trim()) {
            Some(p) if !p.is_empty() => format!(" '{p}'"),
            _ => String::new(),
        };
        let inner = format!("cd {} && {}{}", self.working_dir, agent_tail, prompt_tail);

        match self.transport {
            Transport::Local => inner,
            Transport::Mosh | Transport::Ssh => {
                let bin = self.transport.binary().expect("remote transport");
                let host = self.host.as_deref().unwrap_or("");
                let user = self.user.as_deref().unwrap_or("");
                format!(
                    "{bin} {user}@{host} -- tmux new-session -A -s {sess} '{inner}'",
                    sess = self.tmux_session(),
                )
            }
        }
    }
}

/// Map a runtime label string back to a static slice so `validate()` can
/// return `&'static str`s without allocating.
fn static_label(label: &str) -> &'static str {
    match label {
        "name" => "name",
        "host" => "host",
        "user" => "user",
        "working_dir" => "working_dir",
        "agent" => "agent",
        "agent_args" => "agent_args",
        "agent_prompt" => "agent_prompt",
        "role" => "role",
        "transport" => "transport",
        "project" => "project",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_local() -> SpawnSpec {
        SpawnSpec {
            name: "coordinator".to_string(),
            role: "coordinator".to_string(),
            transport: Transport::Local,
            host: None,
            user: None,
            working_dir: "/Users/mike/work/atn-demo".to_string(),
            project: None,
            agent: "claude".to_string(),
            agent_args: None,
            agent_prompt: None,
            watchdog: None,
        }
    }

    fn worker_hlasm() -> SpawnSpec {
        SpawnSpec {
            name: "worker-hlasm".to_string(),
            role: "worker".to_string(),
            transport: Transport::Mosh,
            host: Some("queenbee".to_string()),
            user: Some("devh1".to_string()),
            working_dir: "/home/devh1/work/hlasm".to_string(),
            project: Some("hlasm".to_string()),
            agent: "codex".to_string(),
            agent_args: None,
            agent_prompt: None,
            watchdog: None,
        }
    }

    #[test]
    fn compose_local() {
        let s = minimal_local();
        assert_eq!(
            s.compose_command(),
            "cd /Users/mike/work/atn-demo && claude"
        );
    }

    #[test]
    fn compose_local_with_args() {
        let mut s = minimal_local();
        s.agent_args = Some("--resume --model sonnet".to_string());
        assert_eq!(
            s.compose_command(),
            "cd /Users/mike/work/atn-demo && claude --resume --model sonnet"
        );
    }

    #[test]
    fn compose_mosh() {
        let s = worker_hlasm();
        assert_eq!(
            s.compose_command(),
            "mosh devh1@queenbee -- tmux new-session -A -s atn-worker-hlasm 'cd /home/devh1/work/hlasm && codex'"
        );
    }

    #[test]
    fn compose_ssh() {
        let mut s = worker_hlasm();
        s.transport = Transport::Ssh;
        s.name = "worker-rpg".to_string();
        s.user = Some("devr1".to_string());
        s.working_dir = "/home/devr1/work/rpg-ii".to_string();
        s.agent = "opencode-z-ai-glm-5".to_string();
        assert_eq!(
            s.compose_command(),
            "ssh devr1@queenbee -- tmux new-session -A -s atn-worker-rpg 'cd /home/devr1/work/rpg-ii && opencode-z-ai-glm-5'"
        );
    }

    #[test]
    fn validate_local_minimum_ok() {
        minimal_local().validate().unwrap();
    }

    #[test]
    fn validate_mosh_requires_host_and_user() {
        let mut s = worker_hlasm();
        s.host = None;
        s.user = None;
        let err = s.validate().unwrap_err();
        assert!(err.contains(&"host"));
        assert!(err.contains(&"user"));
    }

    #[test]
    fn validate_rejects_empty_working_dir() {
        let mut s = minimal_local();
        s.working_dir = "   ".to_string();
        let err = s.validate().unwrap_err();
        assert!(err.contains(&"working_dir"));
    }

    #[test]
    fn validate_rejects_quote_injection() {
        let mut s = minimal_local();
        s.working_dir = "/tmp/foo' ; rm -rf / ;'".to_string();
        let err = s.validate().unwrap_err();
        assert!(err.contains(&"working_dir"));
    }

    #[test]
    fn project_label_from_working_dir() {
        let mut s = minimal_local();
        s.project = None;
        assert_eq!(s.project_label(), "atn-demo");
    }

    #[test]
    fn project_label_explicit_wins() {
        let mut s = minimal_local();
        s.project = Some("PlanRepo".to_string());
        assert_eq!(s.project_label(), "PlanRepo");
    }

    #[test]
    fn tmux_session_name() {
        assert_eq!(worker_hlasm().tmux_session(), "atn-worker-hlasm");
    }

    #[test]
    fn round_trip_json() {
        let s = worker_hlasm();
        let j = serde_json::to_string(&s).unwrap();
        let back: SpawnSpec = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn deserialize_defaults() {
        let j = r#"{"name":"c","working_dir":"/x","agent":"claude"}"#;
        let s: SpawnSpec = serde_json::from_str(j).unwrap();
        assert_eq!(s.transport, Transport::Local);
        assert_eq!(s.role, "worker");
        assert!(s.host.is_none());
    }
}
