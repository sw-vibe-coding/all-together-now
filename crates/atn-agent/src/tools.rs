//! Tool registry + dispatch.
//!
//! The model advertises tools via `ChatRequest.tools` and invokes
//! them by returning `tool_calls` in its response. This module
//! owns both sides: the static `ToolSchema` list we send to the
//! model and the `dispatch` function that executes one `ToolCall`
//! against a workspace sandbox.
//!
//! Step 3 implements `file_read` + `file_write`. Step 4 adds
//! `shell_exec`, `outbox_send`, `inbox_ack`; their schema + handler
//! match arms land in the same place.

use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use atn_core::event::{Priority, PushEvent, PushKind};
use atn_core::inbox::{INBOXES_DIR, OUTBOXES_DIR};
use serde_json::{Value, json};

use crate::llm::{ToolCall, ToolFunctionSchema, ToolSchema};

/// Upper bound on bytes returned from `file_read`. Larger files get
/// a truncation notice so the model knows the content isn't complete.
pub const FILE_READ_MAX: usize = 256 * 1024;

/// Upper bound on bytes accepted by `file_write`. Protects the
/// workspace from a runaway model.
pub const FILE_WRITE_MAX: usize = 1024 * 1024;

/// Wall-clock ceiling on `shell_exec` invocations. After this, we
/// kill the child and return whatever stdout / stderr we captured
/// plus a `"timed out after Ns"` notice.
pub const SHELL_EXEC_TIMEOUT: Duration = Duration::from_secs(30);

/// Max bytes of combined stdout+stderr we send back to the model.
pub const SHELL_OUTPUT_MAX: usize = 4 * 1024;


/// Context passed to each tool invocation. Kept small + Clone so
/// the main loop can share it across iterations without lifetimes.
#[derive(Clone, Debug)]
pub struct ToolCtx {
    /// Sandbox root. Absolute after canonicalization — everything
    /// goes through `sandbox_path` first so `..` escapes are caught.
    pub workspace: PathBuf,
    /// Agent id. Used by `outbox_send` + `inbox_ack` to locate
    /// the per-agent queue directories.
    pub agent_id: String,
    /// ATN coordination root (`.atn` by default). Used to resolve
    /// `<atn_dir>/outboxes/<agent_id>/` and
    /// `<atn_dir>/inboxes/<agent_id>/`.
    pub atn_dir: PathBuf,
    /// Gates the `shell_exec` tool. When false, invoking
    /// `shell_exec` returns a "disabled" message (not an error —
    /// the model should be told it can't run shell).
    pub allow_shell: bool,
}

/// Result of one tool call. `ok=false` signals an error (typically
/// a sandbox violation or a missing file); the content still goes
/// back to the model as a `tool` message so it can correct course.
#[derive(Clone, Debug)]
pub struct ToolResult {
    pub ok: bool,
    /// JSON-stringified payload the caller shoves into
    /// `ChatMessage::tool(...).content`.
    pub content: String,
}

impl ToolResult {
    pub fn ok_value(v: Value) -> Self {
        Self {
            ok: true,
            content: v.to_string(),
        }
    }

    pub fn err<S: Into<String>>(msg: S) -> Self {
        Self {
            ok: false,
            content: json!({ "error": msg.into() }).to_string(),
        }
    }
}

/// Static schema list we attach to every `ChatRequest.tools`.
pub fn tool_schemas() -> Vec<ToolSchema> {
    vec![
        ToolSchema {
            kind: "function".into(),
            function: ToolFunctionSchema {
                name: "file_read".into(),
                description: "Read a file inside the agent's workspace and return its contents. Rejects absolute paths and .. traversal.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Workspace-relative path to read, e.g. 'notes.md' or 'src/lib.rs'."
                        }
                    },
                    "required": ["path"]
                }),
            },
        },
        ToolSchema {
            kind: "function".into(),
            function: ToolFunctionSchema {
                name: "file_write".into(),
                description: "Create or overwrite a file inside the agent's workspace. Creates parent directories as needed.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Workspace-relative path to write."
                        },
                        "content": {
                            "type": "string",
                            "description": "File contents. UTF-8; 1 MiB cap."
                        }
                    },
                    "required": ["path", "content"]
                }),
            },
        },
        ToolSchema {
            kind: "function".into(),
            function: ToolFunctionSchema {
                name: "shell_exec".into(),
                description: "Run a shell command under `/bin/sh -c` in the workspace. 30 s timeout, stdout+stderr truncated to 4 KiB. Disabled unless --allow-shell is set on the agent.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "Shell command line to run."
                        }
                    },
                    "required": ["command"]
                }),
            },
        },
        ToolSchema {
            kind: "function".into(),
            function: ToolFunctionSchema {
                name: "outbox_send".into(),
                description: "Emit a PushEvent to the message router. Valid kinds: feature_request, bug_fix_request, completion_notice, blocked_notice, needs_info, verification_request. Priority defaults to 'normal' if omitted.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "target": {
                            "type": "string",
                            "description": "Target agent id. Omit for broadcast."
                        },
                        "kind": {
                            "type": "string",
                            "description": "PushKind snake_case (e.g. 'completion_notice')."
                        },
                        "summary": {
                            "type": "string",
                            "description": "Short human-readable summary becomes the target's next prompt."
                        },
                        "priority": {
                            "type": "string",
                            "description": "One of 'normal' | 'high' | 'blocking'. Defaults to normal."
                        }
                    },
                    "required": ["kind", "summary"]
                }),
            },
        },
        ToolSchema {
            kind: "function".into(),
            function: ToolFunctionSchema {
                name: "inbox_ack".into(),
                description: "Mark an inbox message as handled by renaming <message_id>.json → .json.done. The main loop auto-acks each message after its chat turn, but use this to explicitly ack mid-run when a single prompt handles multiple inbox entries.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "message_id": {
                            "type": "string",
                            "description": "PushEvent id matching <message_id>.json in the agent's inbox."
                        }
                    },
                    "required": ["message_id"]
                }),
            },
        },
    ]
}

/// Dispatch one `ToolCall`. Returns a `ToolResult` to append to the
/// conversation as a `{role: "tool", content: <json>}` turn.
pub fn dispatch(call: &ToolCall, ctx: &ToolCtx) -> ToolResult {
    let name = call.function.name.as_str();
    // Some local models stringify `arguments` instead of sending an
    // object. Tolerate both shapes by parsing the stringified form
    // before field access.
    let args = normalize_arguments(&call.function.arguments);
    match name {
        "file_read" => tool_file_read(&args, ctx),
        "file_write" => tool_file_write(&args, ctx),
        "shell_exec" => tool_shell_exec(&args, ctx),
        "outbox_send" => tool_outbox_send(&args, ctx),
        "inbox_ack" => tool_inbox_ack(&args, ctx),
        other => ToolResult::err(format!("unknown tool '{other}'")),
    }
}

fn normalize_arguments(raw: &Value) -> Value {
    // If `arguments` came through as a JSON string, re-parse it.
    if let Some(s) = raw.as_str()
        && let Ok(v) = serde_json::from_str::<Value>(s)
    {
        return v;
    }
    raw.clone()
}

fn tool_file_read(args: &Value, ctx: &ToolCtx) -> ToolResult {
    let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
        return ToolResult::err("missing required argument 'path'");
    };
    let resolved = match sandbox_path(&ctx.workspace, path) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(e),
    };
    let data = match std::fs::read(&resolved) {
        Ok(d) => d,
        Err(e) => {
            return ToolResult::err(format!(
                "failed to read {}: {e}",
                resolved.display()
            ));
        }
    };
    let total = data.len();
    let truncated = total > FILE_READ_MAX;
    let slice = if truncated { &data[..FILE_READ_MAX] } else { &data[..] };
    let content = String::from_utf8_lossy(slice).to_string();
    let mut out = json!({
        "path": path,
        "bytes": total,
        "content": content,
    });
    if truncated {
        out["truncated"] = json!(true);
        out["notice"] = json!(format!(
            "file exceeded FILE_READ_MAX ({FILE_READ_MAX} bytes); truncated to prefix"
        ));
    }
    ToolResult::ok_value(out)
}

fn tool_file_write(args: &Value, ctx: &ToolCtx) -> ToolResult {
    let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
        return ToolResult::err("missing required argument 'path'");
    };
    let Some(content) = args.get("content").and_then(|v| v.as_str()) else {
        return ToolResult::err("missing required argument 'content'");
    };
    if content.len() > FILE_WRITE_MAX {
        return ToolResult::err(format!(
            "content exceeds FILE_WRITE_MAX ({FILE_WRITE_MAX} bytes); refusing"
        ));
    }
    let resolved = match sandbox_path(&ctx.workspace, path) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(e),
    };
    if let Some(parent) = resolved.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return ToolResult::err(format!(
            "failed to create parent dir for {}: {e}",
            resolved.display()
        ));
    }
    if let Err(e) = std::fs::write(&resolved, content) {
        return ToolResult::err(format!(
            "failed to write {}: {e}",
            resolved.display()
        ));
    }
    ToolResult::ok_value(json!({
        "path": path,
        "bytes": content.len(),
        "message": format!("wrote {} bytes to {}", content.len(), path),
    }))
}

fn tool_shell_exec(args: &Value, ctx: &ToolCtx) -> ToolResult {
    if !ctx.allow_shell {
        // Not an error — the model needs to know the tool is off
        // so it can stop calling it.
        return ToolResult::ok_value(json!({
            "disabled": true,
            "message": "shell_exec is disabled on this agent (start with --allow-shell to enable)",
        }));
    }
    let Some(command) = args.get("command").and_then(|v| v.as_str()) else {
        return ToolResult::err("missing required argument 'command'");
    };
    let mut child = match Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .current_dir(&ctx.workspace)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return ToolResult::err(format!("spawn /bin/sh: {e}")),
    };

    let deadline = Instant::now() + SHELL_EXEC_TIMEOUT;
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return ToolResult::err(format!("wait for shell: {e}")),
        }
    }
    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => return ToolResult::err(format!("collect shell output: {e}")),
    };

    // Combined stream — models don't usually care which was which.
    let mut combined = String::new();
    combined.push_str(&String::from_utf8_lossy(&output.stdout));
    if !output.stderr.is_empty() {
        if !combined.is_empty() && !combined.ends_with('\n') {
            combined.push('\n');
        }
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    let total = combined.len();
    let (body, truncated) = if total > SHELL_OUTPUT_MAX {
        (
            format!(
                "{prefix}\n…truncated ({total} bytes total)",
                prefix = &combined[..SHELL_OUTPUT_MAX]
            ),
            true,
        )
    } else {
        (combined, false)
    };

    let exit = output.status.code();
    let mut out = json!({
        "command": command,
        "exit_code": exit,
        "bytes": total,
        "output": body,
    });
    if timed_out {
        out["timed_out"] = json!(true);
        out["notice"] = json!(format!(
            "killed after {}s (SHELL_EXEC_TIMEOUT)",
            SHELL_EXEC_TIMEOUT.as_secs()
        ));
    }
    if truncated {
        out["truncated"] = json!(true);
    }
    ToolResult::ok_value(out)
}

fn tool_outbox_send(args: &Value, ctx: &ToolCtx) -> ToolResult {
    let Some(kind_str) = args.get("kind").and_then(|v| v.as_str()) else {
        return ToolResult::err("missing required argument 'kind'");
    };
    let Some(summary) = args.get("summary").and_then(|v| v.as_str()) else {
        return ToolResult::err("missing required argument 'summary'");
    };
    let kind = match parse_push_kind(kind_str) {
        Ok(k) => k,
        Err(e) => return ToolResult::err(e),
    };
    let priority = match args.get("priority").and_then(|v| v.as_str()) {
        Some(p) => match parse_priority(p) {
            Ok(pr) => pr,
            Err(e) => return ToolResult::err(e),
        },
        None => Priority::Normal,
    };
    let target = args
        .get("target")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let event_id = format!(
        "agent-{id}-{millis}",
        id = ctx.agent_id,
        millis = chrono::Utc::now().timestamp_millis()
    );
    let event = PushEvent {
        id: event_id.clone(),
        kind,
        source_agent: ctx.agent_id.clone(),
        source_repo: ".".into(),
        target_agent: target,
        issue_id: None,
        summary: summary.to_string(),
        wiki_link: None,
        priority,
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    let outbox_dir = ctx
        .atn_dir
        .join(OUTBOXES_DIR)
        .join(&ctx.agent_id);
    if let Err(e) = std::fs::create_dir_all(&outbox_dir) {
        return ToolResult::err(format!(
            "create outbox dir {}: {e}",
            outbox_dir.display()
        ));
    }
    let path = outbox_dir.join(format!("{event_id}.json"));
    let json_body = match serde_json::to_string_pretty(&event) {
        Ok(s) => s,
        Err(e) => return ToolResult::err(format!("serialize event: {e}")),
    };
    if let Err(e) = std::fs::write(&path, json_body) {
        return ToolResult::err(format!("write {}: {e}", path.display()));
    }
    ToolResult::ok_value(json!({
        "event_id": event_id,
        "path": path.display().to_string(),
        "message": format!("outbox_send wrote {event_id}"),
    }))
}

fn tool_inbox_ack(args: &Value, ctx: &ToolCtx) -> ToolResult {
    let Some(message_id) = args.get("message_id").and_then(|v| v.as_str()) else {
        return ToolResult::err("missing required argument 'message_id'");
    };
    // Guard against path-injection via message_id.
    if message_id.is_empty()
        || message_id.contains('/')
        || message_id.contains('\\')
        || message_id.contains("..")
    {
        return ToolResult::err(format!(
            "message_id {message_id:?} contains forbidden characters"
        ));
    }
    let inbox_dir = ctx.atn_dir.join(INBOXES_DIR).join(&ctx.agent_id);
    let src = inbox_dir.join(format!("{message_id}.json"));
    let dst = inbox_dir.join(format!("{message_id}.json.done"));
    if !src.exists() {
        return ToolResult::err(format!(
            "inbox message {message_id} not found at {}",
            src.display()
        ));
    }
    if let Err(e) = std::fs::rename(&src, &dst) {
        return ToolResult::err(format!("rename {}: {e}", src.display()));
    }
    ToolResult::ok_value(json!({
        "message_id": message_id,
        "message": format!("acked {message_id}"),
    }))
}

fn parse_push_kind(s: &str) -> Result<PushKind, String> {
    let norm = s.replace('-', "_");
    Ok(match norm.as_str() {
        "feature_request" => PushKind::FeatureRequest,
        "bug_fix_request" => PushKind::BugFixRequest,
        "completion_notice" => PushKind::CompletionNotice,
        "blocked_notice" => PushKind::BlockedNotice,
        "needs_info" => PushKind::NeedsInfo,
        "verification_request" => PushKind::VerificationRequest,
        other => {
            return Err(format!(
                "invalid kind '{other}'; valid values: feature_request, bug_fix_request, completion_notice, blocked_notice, needs_info, verification_request"
            ));
        }
    })
}

fn parse_priority(s: &str) -> Result<Priority, String> {
    match s {
        "normal" => Ok(Priority::Normal),
        "high" => Ok(Priority::High),
        "blocking" => Ok(Priority::Blocking),
        other => Err(format!(
            "invalid priority '{other}'; valid values: normal, high, blocking"
        )),
    }
}

/// Resolve `user_path` against `workspace` and confirm the result
/// stays inside the workspace (no `..` escapes, no absolute paths).
/// The workspace is canonicalized when possible; this rejects paths
/// that `../` out of the sandbox root even if the file doesn't
/// exist yet.
pub fn sandbox_path(workspace: &Path, user_path: &str) -> Result<PathBuf, String> {
    if user_path.is_empty() {
        return Err("path is empty".into());
    }
    let p = Path::new(user_path);
    if p.is_absolute() {
        return Err(format!("absolute paths are not allowed: {user_path:?}"));
    }
    // Reject `..` components outright — path::canonicalize() would
    // also catch most escapes, but for a file that doesn't exist
    // yet we need to refuse before resolving.
    for comp in p.components() {
        match comp {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                return Err(format!("'..' traversal is not allowed: {user_path:?}"));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("path escapes workspace root: {user_path:?}"));
            }
        }
    }
    Ok(workspace.join(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx(workspace: PathBuf) -> ToolCtx {
        ToolCtx {
            workspace,
            agent_id: "demo".into(),
            atn_dir: PathBuf::from(".atn"),
            allow_shell: false,
        }
    }

    #[test]
    fn sandbox_accepts_relative_paths() {
        let ws = PathBuf::from("/tmp/ws");
        assert_eq!(
            sandbox_path(&ws, "notes.md").unwrap(),
            PathBuf::from("/tmp/ws/notes.md")
        );
        assert_eq!(
            sandbox_path(&ws, "src/lib.rs").unwrap(),
            PathBuf::from("/tmp/ws/src/lib.rs")
        );
        assert_eq!(
            sandbox_path(&ws, "./a.txt").unwrap(),
            PathBuf::from("/tmp/ws/./a.txt")
        );
    }

    #[test]
    fn sandbox_rejects_parent_dir() {
        let ws = PathBuf::from("/tmp/ws");
        for bad in ["..", "../escape", "a/../b", "a/../../escape"] {
            let err = sandbox_path(&ws, bad).unwrap_err();
            assert!(err.contains("'..' traversal"), "{bad}: {err}");
        }
    }

    #[test]
    fn sandbox_rejects_absolute() {
        let ws = PathBuf::from("/tmp/ws");
        for bad in ["/etc/passwd", "/tmp/ws/a"] {
            assert!(sandbox_path(&ws, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn sandbox_rejects_empty() {
        assert!(sandbox_path(Path::new("/tmp/ws"), "").is_err());
    }

    #[test]
    fn file_read_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("notes.md"), b"hello agent").unwrap();
        let ctx = test_ctx(tmp.path().to_path_buf());
        let call = ToolCall {
            id: Some("c1".into()),
            kind: "function".into(),
            function: crate::llm::ToolCallFunction {
                name: "file_read".into(),
                arguments: json!({ "path": "notes.md" }),
            },
        };
        let res = dispatch(&call, &ctx);
        assert!(res.ok);
        let v: Value = serde_json::from_str(&res.content).unwrap();
        assert_eq!(v["content"], "hello agent");
        assert_eq!(v["bytes"], 11);
        assert!(v.get("truncated").is_none());
    }

    #[test]
    fn file_read_truncates_oversized() {
        let tmp = tempfile::tempdir().unwrap();
        let big = "a".repeat(FILE_READ_MAX + 100);
        std::fs::write(tmp.path().join("big.txt"), &big).unwrap();
        let ctx = test_ctx(tmp.path().to_path_buf());
        let call = ToolCall {
            id: None,
            kind: "function".into(),
            function: crate::llm::ToolCallFunction {
                name: "file_read".into(),
                arguments: json!({ "path": "big.txt" }),
            },
        };
        let res = dispatch(&call, &ctx);
        assert!(res.ok);
        let v: Value = serde_json::from_str(&res.content).unwrap();
        assert_eq!(v["bytes"], (FILE_READ_MAX + 100) as u64);
        assert_eq!(v["truncated"], true);
        assert_eq!(v["content"].as_str().unwrap().len(), FILE_READ_MAX);
    }

    #[test]
    fn file_write_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = test_ctx(tmp.path().to_path_buf());
        let call = ToolCall {
            id: None,
            kind: "function".into(),
            function: crate::llm::ToolCallFunction {
                name: "file_write".into(),
                arguments: json!({ "path": "out/report.txt", "content": "done" }),
            },
        };
        let res = dispatch(&call, &ctx);
        assert!(res.ok, "{}", res.content);
        let wrote = std::fs::read_to_string(tmp.path().join("out/report.txt")).unwrap();
        assert_eq!(wrote, "done");
    }

    #[test]
    fn file_write_rejects_oversized_content() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = test_ctx(tmp.path().to_path_buf());
        let huge = "x".repeat(FILE_WRITE_MAX + 1);
        let call = ToolCall {
            id: None,
            kind: "function".into(),
            function: crate::llm::ToolCallFunction {
                name: "file_write".into(),
                arguments: json!({ "path": "big.bin", "content": huge }),
            },
        };
        let res = dispatch(&call, &ctx);
        assert!(!res.ok);
        assert!(res.content.contains("FILE_WRITE_MAX"));
    }

    #[test]
    fn file_read_refuses_path_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = test_ctx(tmp.path().to_path_buf());
        let call = ToolCall {
            id: None,
            kind: "function".into(),
            function: crate::llm::ToolCallFunction {
                name: "file_read".into(),
                arguments: json!({ "path": "../../etc/passwd" }),
            },
        };
        let res = dispatch(&call, &ctx);
        assert!(!res.ok);
        assert!(res.content.contains("traversal"));
    }

    #[test]
    fn dispatch_unknown_tool_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = test_ctx(tmp.path().to_path_buf());
        let call = ToolCall {
            id: None,
            kind: "function".into(),
            function: crate::llm::ToolCallFunction {
                name: "eval".into(),
                arguments: json!({}),
            },
        };
        let res = dispatch(&call, &ctx);
        assert!(!res.ok);
        assert!(res.content.contains("unknown tool"));
    }

    #[test]
    fn normalize_arguments_parses_stringified_json() {
        // Some local models serialize arguments as a string literal;
        // normalize_arguments unpacks it so tool handlers see objects.
        let raw = json!(r#"{"path": "notes.md"}"#);
        let got = normalize_arguments(&raw);
        assert_eq!(got["path"], "notes.md");
    }

    #[test]
    fn tool_schemas_includes_every_tool_name() {
        let schemas = tool_schemas();
        let names: Vec<&str> = schemas
            .iter()
            .map(|s| s.function.name.as_str())
            .collect();
        for expected in [
            "file_read",
            "file_write",
            "shell_exec",
            "outbox_send",
            "inbox_ack",
        ] {
            assert!(names.contains(&expected), "missing schema: {expected}");
        }
    }

    fn make_shell_call(cmd: &str) -> ToolCall {
        ToolCall {
            id: None,
            kind: "function".into(),
            function: crate::llm::ToolCallFunction {
                name: "shell_exec".into(),
                arguments: json!({ "command": cmd }),
            },
        }
    }

    #[test]
    fn shell_exec_gated_off_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = test_ctx(tmp.path().to_path_buf());
        assert!(!ctx.allow_shell);
        let res = dispatch(&make_shell_call("echo hi"), &ctx);
        assert!(res.ok);
        let v: Value = serde_json::from_str(&res.content).unwrap();
        assert_eq!(v["disabled"], true);
        assert!(v["message"].as_str().unwrap().contains("--allow-shell"));
    }

    #[test]
    fn shell_exec_when_allowed_captures_stdout() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ctx = test_ctx(tmp.path().to_path_buf());
        ctx.allow_shell = true;
        let res = dispatch(&make_shell_call("echo hello-from-shell"), &ctx);
        assert!(res.ok);
        let v: Value = serde_json::from_str(&res.content).unwrap();
        assert_eq!(v["exit_code"], 0);
        assert!(v["output"].as_str().unwrap().contains("hello-from-shell"));
        assert!(v.get("timed_out").is_none());
    }

    #[test]
    fn shell_exec_captures_nonzero_exit() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ctx = test_ctx(tmp.path().to_path_buf());
        ctx.allow_shell = true;
        let res = dispatch(&make_shell_call("exit 7"), &ctx);
        assert!(res.ok, "{}", res.content);
        let v: Value = serde_json::from_str(&res.content).unwrap();
        assert_eq!(v["exit_code"], 7);
    }

    #[test]
    fn shell_exec_missing_command_argument_is_error() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ctx = test_ctx(tmp.path().to_path_buf());
        ctx.allow_shell = true;
        let call = ToolCall {
            id: None,
            kind: "function".into(),
            function: crate::llm::ToolCallFunction {
                name: "shell_exec".into(),
                arguments: json!({}),
            },
        };
        let res = dispatch(&call, &ctx);
        assert!(!res.ok);
        assert!(res.content.contains("missing"));
    }

    fn make_outbox_call(args: Value) -> ToolCall {
        ToolCall {
            id: None,
            kind: "function".into(),
            function: crate::llm::ToolCallFunction {
                name: "outbox_send".into(),
                arguments: args,
            },
        }
    }

    #[test]
    fn outbox_send_writes_push_event_json() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ctx = test_ctx(tmp.path().to_path_buf());
        ctx.atn_dir = tmp.path().join(".atn");
        let call = make_outbox_call(json!({
            "target": "coord",
            "kind": "completion_notice",
            "summary": "task X done",
            "priority": "high",
        }));
        let res = dispatch(&call, &ctx);
        assert!(res.ok, "{}", res.content);
        let v: Value = serde_json::from_str(&res.content).unwrap();
        let path = v["path"].as_str().unwrap();
        let payload = std::fs::read_to_string(path).unwrap();
        let ev: PushEvent = serde_json::from_str(&payload).unwrap();
        assert_eq!(ev.source_agent, "demo");
        assert_eq!(ev.target_agent.as_deref(), Some("coord"));
        assert_eq!(ev.kind, PushKind::CompletionNotice);
        assert_eq!(ev.priority, Priority::High);
        assert!(ev.id.starts_with("agent-demo-"));
        assert_eq!(ev.summary, "task X done");
    }

    #[test]
    fn outbox_send_accepts_hyphen_kind_alias() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ctx = test_ctx(tmp.path().to_path_buf());
        ctx.atn_dir = tmp.path().join(".atn");
        let call = make_outbox_call(json!({
            "kind": "bug-fix-request",
            "summary": "parser breaks on unicode",
        }));
        let res = dispatch(&call, &ctx);
        assert!(res.ok);
        let v: Value = serde_json::from_str(&res.content).unwrap();
        let ev: PushEvent = serde_json::from_str(
            &std::fs::read_to_string(v["path"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(ev.kind, PushKind::BugFixRequest);
        assert!(ev.target_agent.is_none()); // broadcast
    }

    #[test]
    fn outbox_send_rejects_bad_kind() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ctx = test_ctx(tmp.path().to_path_buf());
        ctx.atn_dir = tmp.path().join(".atn");
        let res = dispatch(
            &make_outbox_call(json!({"kind": "nope", "summary": "x"})),
            &ctx,
        );
        assert!(!res.ok);
        assert!(res.content.contains("feature_request"));
    }

    #[test]
    fn outbox_send_rejects_bad_priority() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ctx = test_ctx(tmp.path().to_path_buf());
        ctx.atn_dir = tmp.path().join(".atn");
        let res = dispatch(
            &make_outbox_call(json!({
                "kind": "needs_info",
                "summary": "x",
                "priority": "urgent",
            })),
            &ctx,
        );
        assert!(!res.ok);
        assert!(res.content.contains("invalid priority"));
    }

    #[test]
    fn inbox_ack_renames_matching_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ctx = test_ctx(tmp.path().to_path_buf());
        ctx.atn_dir = tmp.path().join(".atn");
        let inbox_dir = ctx.atn_dir.join(INBOXES_DIR).join(&ctx.agent_id);
        std::fs::create_dir_all(&inbox_dir).unwrap();
        let src = inbox_dir.join("ev-42.json");
        std::fs::write(&src, "{}").unwrap();
        let call = ToolCall {
            id: None,
            kind: "function".into(),
            function: crate::llm::ToolCallFunction {
                name: "inbox_ack".into(),
                arguments: json!({"message_id": "ev-42"}),
            },
        };
        let res = dispatch(&call, &ctx);
        assert!(res.ok, "{}", res.content);
        assert!(!src.exists());
        assert!(inbox_dir.join("ev-42.json.done").exists());
    }

    #[test]
    fn inbox_ack_missing_file_is_clean_error() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ctx = test_ctx(tmp.path().to_path_buf());
        ctx.atn_dir = tmp.path().join(".atn");
        std::fs::create_dir_all(ctx.atn_dir.join(INBOXES_DIR).join(&ctx.agent_id)).unwrap();
        let call = ToolCall {
            id: None,
            kind: "function".into(),
            function: crate::llm::ToolCallFunction {
                name: "inbox_ack".into(),
                arguments: json!({"message_id": "no-such"}),
            },
        };
        let res = dispatch(&call, &ctx);
        assert!(!res.ok);
        assert!(res.content.contains("not found"));
    }

    #[test]
    fn inbox_ack_rejects_forbidden_ids() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = test_ctx(tmp.path().to_path_buf());
        for bad in ["", "../escape", "a/b", "..", "a\\b"] {
            let call = ToolCall {
                id: None,
                kind: "function".into(),
                function: crate::llm::ToolCallFunction {
                    name: "inbox_ack".into(),
                    arguments: json!({"message_id": bad}),
                },
            };
            let res = dispatch(&call, &ctx);
            assert!(!res.ok, "{bad} should have been rejected");
        }
    }
}
