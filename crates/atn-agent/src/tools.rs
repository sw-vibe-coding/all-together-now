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

use serde_json::{Value, json};

use crate::llm::{ToolCall, ToolFunctionSchema, ToolSchema};

/// Upper bound on bytes returned from `file_read`. Larger files get
/// a truncation notice so the model knows the content isn't complete.
pub const FILE_READ_MAX: usize = 256 * 1024;

/// Upper bound on bytes accepted by `file_write`. Protects the
/// workspace from a runaway model.
pub const FILE_WRITE_MAX: usize = 1024 * 1024;

/// Context passed to each tool invocation. Kept small + Clone so
/// the main loop can share it across iterations without lifetimes.
#[derive(Clone, Debug)]
pub struct ToolCtx {
    /// Sandbox root. Absolute after canonicalization — everything
    /// goes through `sandbox_path` first so `..` escapes are caught.
    pub workspace: PathBuf,
    /// Agent id — threaded in for step-4's outbox writer.
    #[allow(dead_code)]
    pub agent_id: String,
    /// ATN coordination root (`.atn` by default). Step 4 writes
    /// outbox files under `<atn_dir>/outboxes/<agent_id>/`.
    #[allow(dead_code)]
    pub atn_dir: PathBuf,
    /// Flag for step-4's `shell_exec` gate.
    #[allow(dead_code)]
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
    fn tool_schemas_includes_file_read_and_write() {
        let schemas = tool_schemas();
        let names: Vec<&str> = schemas
            .iter()
            .map(|s| s.function.name.as_str())
            .collect();
        assert!(names.contains(&"file_read"));
        assert!(names.contains(&"file_write"));
    }
}
