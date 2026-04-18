use std::time::Duration;

use atn_core::agent::{AgentConfig, AgentId, AgentRole, AgentState};
use atn_core::event::{InputEvent, OutputSignal};
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
