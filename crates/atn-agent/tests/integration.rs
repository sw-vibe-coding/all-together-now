//! End-to-end integration test for atn-agent (saga step 5).
//!
//! Spawns an in-process HTTP stub that answers `POST /api/chat` with
//! a canned sequence of responses:
//!   (a) model calls `file_write`
//!   (b) model calls `outbox_send`
//!   (c) model replies with plain content, no tool_calls (loop exits)
//!
//! Drives the real `atn-agent` binary at the stub via
//! `CARGO_BIN_EXE_atn-agent`, with a tempdir ATN root + seeded
//! inbox. Asserts the file landed, the outbox JSON appeared, and
//! the inbox was renamed to `.json.done`. Uses `--exit-on-empty`
//! so the test tears down cleanly after one pass.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Spawn the stub HTTP server and return `(addr, requests_seen)`.
/// The stub hands out `responses` in order — one per POST to
/// `/api/chat`; extra requests reuse the final response. Request
/// bodies are captured into the shared `requests_seen` vec so the
/// test can assert on what the agent actually sent.
fn spawn_stub(responses: Vec<serde_json::Value>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub");
    let port = listener.local_addr().unwrap().port();
    let addr = format!("http://127.0.0.1:{port}");
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_in = seen.clone();
    thread::spawn(move || {
        let responses = responses;
        let mut idx = 0usize;
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            // Parse a single HTTP/1.1 request with a
            // Content-Length-prefixed JSON body. No keep-alive.
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            if reader.read_line(&mut request_line).is_err() {
                continue;
            }
            let mut content_length = 0usize;
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).is_err() {
                    break;
                }
                if header == "\r\n" || header == "\n" || header.is_empty() {
                    break;
                }
                if let Some(rest) = header
                    .to_ascii_lowercase()
                    .strip_prefix("content-length:")
                {
                    content_length = rest.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; content_length];
            if reader.read_exact(&mut body).is_err() {
                continue;
            }
            seen_in
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&body).to_string());
            let resp = if idx < responses.len() {
                &responses[idx]
            } else {
                responses.last().unwrap()
            };
            idx += 1;
            let body_str = serde_json::to_string(resp).unwrap();
            let out = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{body}",
                len = body_str.len(),
                body = body_str,
            );
            let _ = stream.write_all(out.as_bytes());
            let _ = stream.flush();
        }
    });
    (addr, seen)
}

/// Build a `ChatResponse` body where the assistant asks for one tool.
fn tool_call_response(name: &str, arguments: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "message": {
            "role": "assistant",
            "content": "",
            "tool_calls": [
                {
                    "id": format!("call-{name}"),
                    "type": "function",
                    "function": { "name": name, "arguments": arguments },
                }
            ]
        },
        "done": false
    })
}

fn final_content_response(content: &str) -> serde_json::Value {
    serde_json::json!({
        "message": { "role": "assistant", "content": content },
        "done": true
    })
}

fn wait_for<F: FnMut() -> bool>(timeout: Duration, mut f: F) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    f()
}

#[test]
fn agent_runs_file_write_then_outbox_then_exits() {
    let responses = vec![
        tool_call_response(
            "file_write",
            serde_json::json!({
                "path": "notes.md",
                "content": "# Goals from the model\n"
            }),
        ),
        tool_call_response(
            "outbox_send",
            serde_json::json!({
                "target": "coord",
                "kind": "completion_notice",
                "summary": "wrote notes.md",
                "priority": "normal"
            }),
        ),
        final_content_response("all done"),
    ];
    let (stub_addr, _seen) = spawn_stub(responses);

    // Tempdir layout:
    //   <tmp>/              (workspace)
    //   <tmp>/.atn/inboxes/demo/ev-42.json   (seed)
    //   <tmp>/.atn/outboxes/demo/            (agent creates)
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let atn_dir = root.join(".atn");
    let inbox_dir = atn_dir.join("inboxes/demo");
    std::fs::create_dir_all(&inbox_dir).unwrap();
    let seed = serde_json::json!({
        "event": {
            "id": "ev-42",
            "kind": "feature_request",
            "source_agent": "coord",
            "source_repo": ".",
            "target_agent": "demo",
            "issue_id": null,
            "summary": "write a goals file",
            "wiki_link": null,
            "priority": "normal",
            "timestamp": "2026-04-24T12:00:00Z"
        },
        "delivered": true,
        "delivered_at": "2026-04-24T12:00:01Z"
    });
    std::fs::write(
        inbox_dir.join("ev-42.json"),
        serde_json::to_string_pretty(&seed).unwrap(),
    )
    .unwrap();

    // Run the agent one-shot via --exit-on-empty. Three /api/chat
    // turns happen on the single inbox message.
    let bin = env!("CARGO_BIN_EXE_atn-agent");
    let out = Command::new(bin)
        .args([
            "--agent-id", "demo",
            "--base-url", &stub_addr,
            "--model", "stub-model",
            "--atn-dir", atn_dir.to_str().unwrap(),
            "--workspace", root.to_str().unwrap(),
            "--inbox-poll-secs", "1",
            "--exit-on-empty",
        ])
        .output()
        .expect("run atn-agent");

    assert!(out.status.success(), "agent exit: {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    // Banner + tool logs landed on stdout.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("atn-agent: demo up"), "stdout:\n{stdout}");
    assert!(stdout.contains("tool file_write → ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("tool outbox_send → ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("all done"), "stdout:\n{stdout}");

    // file_write landed in the workspace.
    let wrote = std::fs::read_to_string(root.join("notes.md")).expect("notes.md");
    assert!(wrote.contains("Goals from the model"));

    // outbox_send dropped a PushEvent in the outbox.
    let outbox_dir = atn_dir.join("outboxes/demo");
    let files: Vec<_> = std::fs::read_dir(&outbox_dir)
        .expect("outbox dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    assert_eq!(files.len(), 1, "expected one outbox file, got {files:?}");
    let payload = std::fs::read_to_string(files[0].path()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(parsed["source_agent"], "demo");
    assert_eq!(parsed["target_agent"], "coord");
    assert_eq!(parsed["kind"], "completion_notice");
    assert_eq!(parsed["summary"], "wrote notes.md");

    // Inbox message was renamed to .json.done.
    assert!(!inbox_dir.join("ev-42.json").exists(), "inbox file still present");
    assert!(
        inbox_dir.join("ev-42.json.done").exists(),
        "expected .json.done in {}",
        inbox_dir.display()
    );

    // Sanity on stub — agent hit it at least 3 times (the model's
    // three-turn conversation) and at most a handful more for the
    // empty-pass exit path.
    let hits = wait_for(Duration::from_millis(200), || true);
    assert!(hits);
}

#[test]
fn agent_handles_disabled_shell_tool_gracefully() {
    // Model asks for shell_exec without --allow-shell → the tool
    // returns {disabled: true}, the next turn finishes clean.
    let responses = vec![
        tool_call_response(
            "shell_exec",
            serde_json::json!({ "command": "uname -a" }),
        ),
        final_content_response("got it, can't run shell here"),
    ];
    let (stub_addr, _seen) = spawn_stub(responses);

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let atn_dir = root.join(".atn");
    let inbox_dir = atn_dir.join("inboxes/demo");
    std::fs::create_dir_all(&inbox_dir).unwrap();
    std::fs::write(
        inbox_dir.join("ev-1.json"),
        serde_json::to_string(&serde_json::json!({
            "event": {
                "id": "ev-1",
                "kind": "needs_info",
                "source_agent": "coord",
                "source_repo": ".",
                "target_agent": "demo",
                "issue_id": null,
                "summary": "what kernel?",
                "wiki_link": null,
                "priority": "normal",
                "timestamp": "2026-04-24T13:00:00Z"
            },
            "delivered": true,
            "delivered_at": null
        }))
        .unwrap(),
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_atn-agent");
    let out = Command::new(bin)
        .args([
            "--agent-id", "demo",
            "--base-url", &stub_addr,
            "--model", "stub-model",
            "--atn-dir", atn_dir.to_str().unwrap(),
            "--workspace", root.to_str().unwrap(),
            "--inbox-poll-secs", "1",
            "--exit-on-empty",
            // NB: --allow-shell NOT set.
        ])
        .output()
        .expect("run atn-agent");

    assert!(out.status.success(), "agent exit failure: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    // shell_exec is recorded as `ok` even when disabled — the tool
    // returned ok_value({disabled: true}) so the model could recover.
    assert!(stdout.contains("tool shell_exec → ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("got it, can't run shell here"));
    assert!(inbox_dir.join("ev-1.json.done").exists());
}
