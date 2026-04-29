use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{RwLock, broadcast};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use atn_core::agent::AgentState;
use atn_core::event::OutputSignal;

use crate::watchdog::WatchdogState;

const IDLE_TIMEOUT: Duration = Duration::from_secs(5);
/// How often the tracker polls the watchdog when the PTY is silent.
/// Short enough that `stalled_for_secs` in the state endpoint stays
/// close to real time, long enough to be cheap.
const WATCHDOG_POLL_INTERVAL: Duration = Duration::from_secs(1);
const PROMPT_MARKER: &[u8] = b"__ATN_READY__>";
/// Substrings in PTY output that classify the agent as awaiting human
/// input.
///
/// Generic shell prompts: `? ` (question + space — read -p style),
/// `(y/n)`, `[Y/n]`, `[y/N]` (apt/git/bash confirmation forms).
///
/// Claude Code permission dialog: claude renders `to proceed?` (no
/// trailing space — newline + ANSI escapes follow) and emits an
/// OSC 9 system notification `\x1b]9;Claude needs your permission`
/// when waiting on a tool-use confirmation. Match either the inline
/// dialog text or the OSC 9 prefix.
///
/// Codex / opencode: trust dialogs include the literal phrase
/// `1. Yes, continue` / `1. Yes, proceed`. Match the menu prefix
/// since the dialog is alt-screen and the surrounding text is full
/// of ANSI escapes.
const QUESTION_MARKERS: &[&[u8]] = &[
    b"? ",
    b"(y/n)",
    b"[Y/n]",
    b"[y/N]",
    // Claude Code
    b"to proceed?",
    b"\x1b]9;Claude needs your permission",
    // Codex / opencode trust dialogs
    b"1. Yes, continue",
    b"1. Yes, proceed",
];

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
    watchdog: Arc<RwLock<WatchdogState>>,
    agent_id: String,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut last_output = Instant::now();

        loop {
            // Pick the sooner of (a) the IDLE_TIMEOUT deadline and
            // (b) a watchdog-poll tick so stall detection runs at
            // 1-second granularity regardless of PTY traffic.
            let watchdog_tick = Instant::now() + WATCHDOG_POLL_INTERVAL;
            let idle_deadline = last_output + IDLE_TIMEOUT;
            let next_tick = watchdog_tick.min(idle_deadline);

            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(OutputSignal::Bytes(ref data)) => {
                            last_output = Instant::now();
                            let new_state = classify_output(data);
                            let mut s = state.write().await;
                            // Only update if still in a trackable state
                            let prev = s.clone();
                            match *s {
                                AgentState::Starting
                                | AgentState::Running
                                | AgentState::Idle
                                | AgentState::AwaitingHumanInput => {
                                    *s = new_state.clone();
                                }
                                _ => {}
                            }
                            drop(s);
                            update_watchdog_on_output(&watchdog, last_output.into_std(), &prev, &new_state).await;
                        }
                        Ok(OutputSignal::PromptReady) => {
                            let mut s = state.write().await;
                            *s = AgentState::Idle;
                            drop(s);
                            watchdog.write().await.on_leaving_running();
                        }
                        Ok(OutputSignal::QuestionDetected { .. }) => {
                            let mut s = state.write().await;
                            *s = AgentState::AwaitingHumanInput;
                            drop(s);
                            watchdog.write().await.on_leaving_running();
                        }
                        Ok(OutputSignal::IdleDetected) => {
                            let mut s = state.write().await;
                            if *s == AgentState::Running {
                                *s = AgentState::Idle;
                                drop(s);
                                watchdog.write().await.on_leaving_running();
                            }
                        }
                        Ok(OutputSignal::PushEvent(_)) => {}
                        Ok(OutputSignal::Disconnected) => {
                            let mut s = state.write().await;
                            *s = AgentState::Disconnected;
                            drop(s);
                            watchdog.write().await.on_leaving_running();
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::debug!("State tracker lagged by {n} messages");
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = tokio::time::sleep_until(next_tick) => {
                    // Idle-detection: long gap in Running → Idle.
                    let now = Instant::now();
                    if now >= idle_deadline {
                        let mut s = state.write().await;
                        if *s == AgentState::Running {
                            *s = AgentState::Idle;
                            drop(s);
                            watchdog.write().await.on_leaving_running();
                            continue;
                        }
                    }
                    // Stall check: state is still Running but the
                    // PTY has been quiet long enough to cross
                    // `stall_secs`. One-shot per stall event so the
                    // log doesn't spam.
                    let cur_state = state.read().await.clone();
                    let mut w = watchdog.write().await;
                    if w.check_stall(now.into_std(), &cur_state) {
                        tracing::warn!(
                            "agent {agent_id} stalled: no output for {}s while running",
                            w.config.stall_secs
                        );
                    }
                }
            }
        }
    })
}

async fn update_watchdog_on_output(
    watchdog: &Arc<RwLock<WatchdogState>>,
    now: std::time::Instant,
    prev_state: &AgentState,
    next_state: &AgentState,
) {
    let mut w = watchdog.write().await;
    w.on_output(now);
    // Track Running-window entry/exit so max_running_secs (step 6)
    // has a start time.
    let was_running = matches!(prev_state, AgentState::Running);
    let is_running = matches!(next_state, AgentState::Running);
    match (was_running, is_running) {
        (false, true) => w.on_entering_running(now),
        (true, false) => w.on_leaving_running(),
        _ => {}
    }
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

    #[test]
    fn classify_claude_to_proceed_dialog() {
        // Claude's permission dialog has "to proceed?" with no trailing
        // space — newline + ANSI escapes follow. The pre-fix classifier
        // missed this pattern.
        let data = b"\x1b[1mDo you want to proceed?\x1b[0m\n  1. Yes\n  2. No";
        assert_eq!(classify_output(data), AgentState::AwaitingHumanInput);
    }

    #[test]
    fn classify_claude_osc9_permission_notification() {
        // Claude emits an OSC 9 system notification when it wants tool
        // permission. The escape sequence is `\x1b]9;<message>\x07`.
        let data = b"\x1b]9;Claude needs your permission to use Bash\x07";
        assert_eq!(classify_output(data), AgentState::AwaitingHumanInput);
    }

    #[test]
    fn classify_codex_trust_dialog() {
        // Codex / opencode trust dialogs render a numbered menu that
        // we key off of. Lots of surrounding ANSI; match the menu line.
        let data = b"\x1b[2J\x1b[H Trust this directory? \x1b[?25l\n  1. Yes, continue\n  2. No, quit\n";
        assert_eq!(classify_output(data), AgentState::AwaitingHumanInput);
    }
}
