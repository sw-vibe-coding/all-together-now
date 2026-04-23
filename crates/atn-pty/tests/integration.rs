use std::time::Duration;

use atn_core::agent::{AgentConfig, AgentId, AgentRole, AgentState};
use atn_core::event::{CannedAction, InputEvent, OutputSignal};
use atn_pty::session::PtySession;

fn test_config(tmp: &std::path::Path) -> AgentConfig {
    AgentConfig {
        id: AgentId("test-agent".to_string()),
        name: "Test Agent".to_string(),
        repo_path: tmp.to_path_buf(),
        role: AgentRole::Developer,
        setup_commands: vec![],
        launch_command: String::new(),
    }
}

/// Collect output bytes from a broadcast receiver until a timeout or a match is found.
async fn collect_output_until(
    mut rx: tokio::sync::broadcast::Receiver<OutputSignal>,
    needle: &str,
    timeout: Duration,
) -> String {
    let mut collected = String::new();
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(OutputSignal::Bytes(data))) => {
                if let Ok(s) = std::str::from_utf8(&data) {
                    collected.push_str(s);
                    if collected.contains(needle) {
                        break;
                    }
                }
            }
            Ok(Ok(_)) => {}      // other signals
            Ok(Err(_)) => break, // channel closed or lagged
            Err(_) => break,     // timeout
        }
    }

    collected
}

#[tokio::test]
async fn spawn_and_echo() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());

    let mut session = PtySession::spawn(&config, None).unwrap();
    let rx = session.output_receiver();

    // Wait for bash to initialize.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send echo command.
    session
        .send_input(InputEvent::HumanText {
            text: "echo ATN_TEST_MARKER".to_string(),
        })
        .await
        .unwrap();

    let output = collect_output_until(rx, "ATN_TEST_MARKER", Duration::from_secs(5)).await;
    assert!(
        output.contains("ATN_TEST_MARKER"),
        "Expected output to contain 'ATN_TEST_MARKER', got: {output}"
    );

    session.shutdown().await.unwrap();
}

#[tokio::test]
async fn ctrl_c_interrupt() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());

    let mut session = PtySession::spawn(&config, None).unwrap();
    let rx = session.output_receiver();

    // Wait for bash to initialize.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Start a long-running command.
    session
        .send_input(InputEvent::HumanText {
            text: "sleep 300".to_string(),
        })
        .await
        .unwrap();

    // Give it a moment to start.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Send Ctrl-C.
    session.send_ctrl_c().await.unwrap();

    // The shell prompt should return — look for the ATN prompt marker.
    let output = collect_output_until(rx, "__ATN_READY__", Duration::from_secs(5)).await;
    assert!(
        output.contains("__ATN_READY__"),
        "Expected prompt to return after Ctrl-C, got: {output}"
    );

    session.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_clean() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());

    let mut session = PtySession::spawn(&config, None).unwrap();

    // Wait for bash to initialize.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Start a long-running command.
    session
        .send_input(InputEvent::HumanText {
            text: "sleep 300".to_string(),
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;

    // Shutdown should complete without hanging.
    let result = tokio::time::timeout(Duration::from_secs(10), session.shutdown()).await;

    assert!(result.is_ok(), "Shutdown timed out");
    assert!(result.unwrap().is_ok(), "Shutdown returned error");

    // State should be Disconnected.
    let state = session.state();
    let s = state.read().await;
    assert_eq!(*s, AgentState::Disconnected);
}

#[tokio::test]
async fn transcript_logging() {
    let tmp = tempfile::tempdir().unwrap();
    let log_dir = tmp.path().join("logs");
    let config = test_config(tmp.path());

    let mut session = PtySession::spawn(&config, Some(log_dir.clone())).unwrap();

    // Wait for bash to initialize.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send a command that produces known output.
    session
        .send_input(InputEvent::HumanText {
            text: "echo TRANSCRIPT_TEST_123".to_string(),
        })
        .await
        .unwrap();

    // Wait for output to be logged.
    tokio::time::sleep(Duration::from_secs(1)).await;

    session.shutdown().await.unwrap();

    // Give the transcript writer time to flush.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Check that transcript.log was created and has content.
    let transcript_path = log_dir.join("test-agent").join("transcript.log");
    assert!(
        transcript_path.exists(),
        "transcript.log should exist at {transcript_path:?}"
    );

    let transcript = std::fs::read_to_string(&transcript_path).unwrap();
    assert!(
        transcript.contains("TRANSCRIPT_TEST_123"),
        "Transcript should contain our echo output"
    );
}

#[tokio::test]
async fn session_manager_lifecycle() {
    use atn_pty::manager::SessionManager;

    let tmp = tempfile::tempdir().unwrap();
    let mut mgr = SessionManager::new(None);

    assert!(mgr.is_empty());

    let config = AgentConfig {
        id: AgentId("mgr-test".to_string()),
        name: "Manager Test".to_string(),
        repo_path: tmp.path().to_path_buf(),
        role: AgentRole::Developer,
        setup_commands: vec![],
        launch_command: String::new(),
    };

    let id = mgr.spawn_agent(config).unwrap();
    assert_eq!(mgr.len(), 1);
    assert!(mgr.get_session(&id).is_ok());

    // Wait for init.
    tokio::time::sleep(Duration::from_millis(500)).await;

    mgr.shutdown_agent(&id).await.unwrap();
    assert!(mgr.is_empty());

    // Shutting down a removed agent should error.
    assert!(mgr.shutdown_agent(&id).await.is_err());
}

/// Run the composed shell command from a SpawnSpec through a fake `mosh`/`ssh`
/// on PATH and return the recorded argv lines. Asserts exit code 0.
fn run_with_fake_transport(spec: &atn_core::spawn_spec::SpawnSpec) -> Vec<String> {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::process::Command;

    let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let fake_mosh = workspace_root.join("tools").join("fake-mosh");
    assert!(
        fake_mosh.exists(),
        "tools/fake-mosh missing at {}",
        fake_mosh.display()
    );

    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    // Both transports back to the same recorder.
    symlink(&fake_mosh, bin_dir.join("mosh")).unwrap();
    symlink(&fake_mosh, bin_dir.join("ssh")).unwrap();

    let log_path = tmp.path().join("argv.log");
    let composed = spec.compose_command();

    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{old_path}", bin_dir.display());

    let out = Command::new("bash")
        .arg("-c")
        .arg(&composed)
        .env("PATH", new_path)
        .env("ATN_FAKE_MOSH_LOG", &log_path)
        .env("ATN_FAKE_MOSH_BANNER", "fake transport banner")
        .output()
        .expect("bash -c failed");
    assert!(
        out.status.success(),
        "bash -c {composed:?} exited {}: stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    let log = fs::read_to_string(&log_path).unwrap_or_default();
    log.lines()
        .filter_map(|l| l.strip_prefix("arg=").map(|s| s.to_string()))
        .collect()
}

#[tokio::test]
async fn remote_mosh_transport_records_expected_argv() {
    use atn_core::spawn_spec::{SpawnSpec, Transport};

    let spec = SpawnSpec {
        name: "worker-hlasm".to_string(),
        role: "worker".to_string(),
        transport: Transport::Mosh,
        host: Some("queenbee".to_string()),
        user: Some("devh1".to_string()),
        working_dir: "/home/devh1/work/hlasm".to_string(),
        project: Some("hlasm".to_string()),
        agent: "codex".to_string(),
        agent_args: None,
    };

    let argv = run_with_fake_transport(&spec);
    assert_eq!(
        argv,
        vec![
            "devh1@queenbee",
            "--",
            "tmux",
            "new-session",
            "-A",
            "-s",
            "atn-worker-hlasm",
            "cd /home/devh1/work/hlasm && codex",
        ]
    );
}

#[tokio::test]
async fn remote_ssh_transport_records_expected_argv() {
    use atn_core::spawn_spec::{SpawnSpec, Transport};

    let spec = SpawnSpec {
        name: "worker-rpg".to_string(),
        role: "worker".to_string(),
        transport: Transport::Ssh,
        host: Some("queenbee".to_string()),
        user: Some("devr1".to_string()),
        working_dir: "/home/devr1/work/rpg-ii".to_string(),
        project: None,
        agent: "opencode-z-ai-glm-5".to_string(),
        agent_args: Some("--resume".to_string()),
    };

    let argv = run_with_fake_transport(&spec);
    assert_eq!(
        argv,
        vec![
            "devr1@queenbee",
            "--",
            "tmux",
            "new-session",
            "-A",
            "-s",
            "atn-worker-rpg",
            "cd /home/devr1/work/rpg-ii && opencode-z-ai-glm-5 --resume",
        ]
    );
}

#[tokio::test]
async fn pty_exit_sets_disconnected_state() {
    use atn_core::agent::{AgentConfig, AgentId, AgentRole};
    use atn_pty::manager::SessionManager;

    let tmp = tempfile::tempdir().unwrap();
    let mut mgr = SessionManager::new(None);

    // Launch command `exit 0` makes the shell quit immediately, causing EOF
    // on the PTY reader and flipping state to Disconnected.
    let config = AgentConfig {
        id: AgentId("exit-quick".to_string()),
        name: "exit-quick".to_string(),
        repo_path: tmp.path().to_path_buf(),
        role: AgentRole::Developer,
        setup_commands: vec![],
        launch_command: "exit 0".to_string(),
    };

    let id = mgr.spawn_agent(config).unwrap();

    // Give the session a moment to type `exit 0` and for the shell to close.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
    loop {
        let state_arc = {
            let sess = mgr.get_session(&id).unwrap();
            sess.state()
        };
        let s = state_arc.read().await.clone();
        if matches!(s, atn_core::agent::AgentState::Disconnected) {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("agent never transitioned to Disconnected; last state = {s:?}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let _ = mgr.shutdown_agent(&id).await;
}

#[tokio::test]
async fn empty_start_loads_zero_agents() {
    use atn_core::config::load_project_config;
    use atn_pty::manager::SessionManager;

    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("agents.toml");
    std::fs::write(
        &cfg_path,
        r#"[project]
name = "empty-start-test"
"#,
    )
    .unwrap();

    let project_config = load_project_config(&cfg_path).expect("empty agents.toml parses");
    assert_eq!(project_config.project.name, "empty-start-test");
    assert!(project_config.agents.is_empty());

    let mut mgr = SessionManager::new(None);
    for entry in &project_config.agents {
        let _ = mgr.spawn_agent(entry.to_agent_config(tmp.path()));
    }
    assert!(mgr.is_empty());
    assert_eq!(mgr.len(), 0);
}

// --- Canned-action shell-escape regression tests (ops-polish step 1) ---
//
// These verify that CannedAction::ReadWiki / Ack pass their page + id
// arguments through `atn_core::shell::shell_escape` before the line hits
// the PTY. Without escaping, `(priority: High)` triggers a bash parser
// error because `(` starts a subshell. With escaping, bash sees a single
// quoted word and tries to run `coord` (which fails with a normal
// "command not found", no syntax error).

/// Drain the output channel for `window`, return everything seen. Used
/// by the regression tests below — they need the full post-action
/// buffer rather than an early exit on the first prompt marker.
async fn drain_output(
    mut rx: tokio::sync::broadcast::Receiver<OutputSignal>,
    window: Duration,
) -> String {
    let mut collected = String::new();
    let deadline = tokio::time::Instant::now() + window;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(OutputSignal::Bytes(data))) => {
                if let Ok(s) = std::str::from_utf8(&data) {
                    collected.push_str(s);
                }
            }
            Ok(Ok(_)) => {}
            Ok(Err(_)) | Err(_) => break,
        }
    }
    collected
}

#[tokio::test]
async fn canned_read_wiki_quotes_metacharacters() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());
    let mut session = PtySession::spawn(&config, None).unwrap();
    // Let bash finish the initial `export PS1=...` setup so its echo
    // doesn't land in the buffer we're about to inspect.
    tokio::time::sleep(Duration::from_millis(800)).await;
    let rx = session.output_receiver();

    // The canonical regression case: parens + colon + spaces.
    session
        .send_input(InputEvent::Action {
            action: CannedAction::ReadWiki {
                page: "Requests (priority: High)".to_string(),
            },
        })
        .await
        .unwrap();

    let output = drain_output(rx, Duration::from_secs(2)).await;
    assert!(
        !output.contains("syntax error"),
        "bash should not have parsed the parens as a subshell; got: {output}"
    );
    assert!(
        output.contains("coord: command not found") || output.contains("coord: not found"),
        "expected bash to try to run `coord` as a single word; got: {output}"
    );
    session.shutdown().await.unwrap();
}

#[tokio::test]
async fn canned_ack_quotes_single_quotes() {
    // request_id that itself contains a single quote. Without the
    // `'\''` dance the shell would see an unterminated quoted string.
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path());
    let mut session = PtySession::spawn(&config, None).unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    let rx = session.output_receiver();

    session
        .send_input(InputEvent::Action {
            action: CannedAction::Ack {
                request_id: "REQ-can't-touch-this".to_string(),
            },
        })
        .await
        .unwrap();

    let output = drain_output(rx, Duration::from_secs(2)).await;
    assert!(
        !output.contains("syntax error") && !output.contains("unexpected EOF"),
        "shell should handle quoted single-quote cleanly; got: {output}"
    );
    assert!(
        output.contains("coord: command not found") || output.contains("coord: not found"),
        "expected bash to try to run `coord` as a single word; got: {output}"
    );
    session.shutdown().await.unwrap();
}
