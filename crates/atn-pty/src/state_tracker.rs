use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{RwLock, broadcast};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use atn_core::agent::AgentState;
use atn_core::event::OutputSignal;

const IDLE_TIMEOUT: Duration = Duration::from_secs(5);
const PROMPT_MARKER: &[u8] = b"__ATN_READY__>";
/// Claude Code displays a question mark or "?" prompt patterns when awaiting input.
const QUESTION_MARKERS: &[&[u8]] = &[b"? ", b"(y/n)", b"[Y/n]", b"[y/N]"];

/// Spawns a task that monitors PTY output and updates agent state accordingly.
///
/// State transitions:
/// - Bytes received with prompt marker → Idle
/// - Bytes received with question markers → AwaitingHumanInput
/// - Bytes received (general) → Running
/// - No output for IDLE_TIMEOUT → Idle (from Running only)
pub fn spawn_state_tracker(
    mut rx: broadcast::Receiver<OutputSignal>,
    state: Arc<RwLock<AgentState>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut last_output = Instant::now();

        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(OutputSignal::Bytes(ref data)) => {
                            last_output = Instant::now();
                            let new_state = classify_output(data);
                            let mut s = state.write().await;
                            // Only update if still in a trackable state
                            match *s {
                                AgentState::Starting
                                | AgentState::Running
                                | AgentState::Idle
                                | AgentState::AwaitingHumanInput => {
                                    *s = new_state;
                                }
                                _ => {}
                            }
                        }
                        Ok(OutputSignal::PromptReady) => {
                            let mut s = state.write().await;
                            *s = AgentState::Idle;
                        }
                        Ok(OutputSignal::QuestionDetected { .. }) => {
                            let mut s = state.write().await;
                            *s = AgentState::AwaitingHumanInput;
                        }
                        Ok(OutputSignal::IdleDetected) => {
                            let mut s = state.write().await;
                            if *s == AgentState::Running {
                                *s = AgentState::Idle;
                            }
                        }
                        Ok(OutputSignal::PushEvent(_)) => {}
                        Ok(OutputSignal::Disconnected) => {
                            let mut s = state.write().await;
                            *s = AgentState::Disconnected;
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::debug!("State tracker lagged by {n} messages");
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = tokio::time::sleep_until(last_output + IDLE_TIMEOUT) => {
                    let mut s = state.write().await;
                    if *s == AgentState::Running {
                        *s = AgentState::Idle;
                    }
                }
            }
        }
    })
}

/// Classify raw PTY output bytes into an agent state.
fn classify_output(data: &[u8]) -> AgentState {
    // Check for prompt marker first (most specific)
    if contains_bytes(data, PROMPT_MARKER) {
        return AgentState::Idle;
    }

    // Check for question/input-awaiting patterns
    for marker in QUESTION_MARKERS {
        if contains_bytes(data, marker) {
            return AgentState::AwaitingHumanInput;
        }
    }

    AgentState::Running
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_prompt_as_idle() {
        let data = b"some output\r\n__ATN_READY__> ";
        assert_eq!(classify_output(data), AgentState::Idle);
    }

    #[test]
    fn classify_question_as_awaiting() {
        let data = b"Do you want to proceed? (y/n) ";
        assert_eq!(classify_output(data), AgentState::AwaitingHumanInput);
    }

    #[test]
    fn classify_general_output_as_running() {
        let data = b"compiling crate foo...";
        assert_eq!(classify_output(data), AgentState::Running);
    }
}
