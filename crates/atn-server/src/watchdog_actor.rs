//! Background task that turns the per-agent stall signal (set by
//! `atn_pty::state_tracker` into `WatchdogState.stalled`) into real
//! consequences: Ctrl-C the agent, post a `blocked_notice` for the
//! coordinator if that doesn't shake it loose, and escalate if the
//! agent exceeds its `max_running_secs` ceiling.
//!
//! This is the policy layer that step 5 left open. Intentionally kept
//! out of `atn-pty` so the PTY crate stays observation-only.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use atn_core::agent::AgentId;
use atn_core::event::{InputEvent, Priority, PushEvent, PushKind};
use atn_core::inbox::{ATN_DIR_NAME, OUTBOXES_DIR};
use atn_pty::manager::SessionManager;
use atn_pty::watchdog::WatchdogState;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

/// How often the actor polls the fleet. 1 s keeps the detection
/// latency snappy without being spammy.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Per-agent action bookkeeping. Cleared whenever the agent recovers
/// (watchdog.stalled flips back to false).
#[derive(Default, Debug)]
struct ActionState {
    /// When we last sent Ctrl-C for the current stall event. `None`
    /// means we haven't acted on this stall yet.
    last_ctrl_c_at: Option<Instant>,
    /// Whether we've already posted a `blocked_notice` for this stall.
    blocked_notice_posted: bool,
    /// Whether we've posted a `blocked_notice` for exceeding the
    /// `max_running_secs` ceiling in the current running window.
    running_escalated: bool,
}

/// Spawn the watchdog action loop.
pub fn spawn_watchdog_actor(
    manager: Arc<Mutex<SessionManager>>,
    base_dir: PathBuf,
    coordinator_hint: Arc<Mutex<Option<String>>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut states: HashMap<String, ActionState> = HashMap::new();
        loop {
            tokio::time::sleep(POLL_INTERVAL).await;

            // Snapshot per-agent (id, watchdog-handle, role) while
            // briefly holding the manager lock. We'll read the
            // watchdog state + act without the lock held.
            let snapshot: Vec<(String, Arc<RwLock<WatchdogState>>, String)> = {
                let mgr = manager.lock().await;
                mgr.agent_ids()
                    .into_iter()
                    .filter_map(|id| {
                        mgr.get_session(id).ok().map(|sess| {
                            (
                                id.0.clone(),
                                sess.watchdog(),
                                sess.role().to_string(),
                            )
                        })
                    })
                    .collect()
            };

            // Identify the coordinator (if any) from the live roles.
            // Cache it so the first hit wins — we only need a rough
            // target for blocked_notice routing.
            let coordinator_id = {
                let cached = coordinator_hint.lock().await.clone();
                if cached.is_some() {
                    cached
                } else {
                    let found = snapshot
                        .iter()
                        .find(|(_, _, role)| role.eq_ignore_ascii_case("coordinator"))
                        .map(|(id, _, _)| id.clone());
                    if found.is_some() {
                        *coordinator_hint.lock().await = found.clone();
                    }
                    found
                }
            };

            for (id, watchdog, _role) in snapshot.into_iter() {
                let now = Instant::now();
                let (is_stalled, stall_secs, stalled_for, running_for, max_running_secs, stall_count) = {
                    let w = watchdog.read().await;
                    (
                        w.stalled,
                        w.config.stall_secs,
                        w.stalled_for_secs(now).unwrap_or(0),
                        w.running_for_secs(now),
                        w.config.max_running_secs,
                        w.stall_count_in_run,
                    )
                };

                // Agent has left the `Running` window entirely
                // (stall_count resets on `on_leaving_running`): clear
                // our action bookkeeping so the next run starts fresh.
                if stall_count == 0 {
                    states.remove(&id);
                    // Still fire max-running escalation if the agent
                    // manages to stay in Running without being
                    // flagged stalled but beyond the ceiling.
                    if let (Some(max), Some(elapsed)) = (max_running_secs, running_for)
                        && elapsed >= max
                    {
                        let action = states.entry(id.clone()).or_default();
                        if !action.running_escalated {
                            post_blocked_notice(
                                &base_dir,
                                &id,
                                coordinator_id.as_deref(),
                                &format!(
                                    "agent running continuously for {elapsed}s (max_running_secs = {max}); investigate"
                                ),
                            )
                            .await;
                            action.running_escalated = true;
                            tracing::warn!(
                                "watchdog: posted blocked_notice for {id} — running_for_secs {elapsed}s exceeded max {max}s"
                            );
                        }
                    }
                    continue;
                }

                let action = states.entry(id.clone()).or_default();

                // First stall event in this Running window: send Ctrl-C.
                if stall_count >= 1 && action.last_ctrl_c_at.is_none() {
                    if let Err(e) = send_ctrl_c(&manager, &id).await {
                        tracing::warn!("watchdog: failed to Ctrl-C {id}: {e}");
                    } else {
                        action.last_ctrl_c_at = Some(now);
                        tracing::warn!(
                            "watchdog.ctrl_c: sent Ctrl-C to {id} (stalled for {stalled_for}s, stall_secs = {stall_secs})"
                        );
                    }
                }

                // Second stall event in the same Running window: the
                // Ctrl-C didn't stick. Post a `blocked_notice` once,
                // target the coordinator if we can identify one.
                if stall_count >= 2 && !action.blocked_notice_posted {
                    // Only worth escalating if the stall is current
                    // (re-fired and is still stuck) or at least freshly
                    // noticed.
                    if is_stalled {
                        post_blocked_notice(
                            &base_dir,
                            &id,
                            coordinator_id.as_deref(),
                            &format!(
                                "agent stalled {stall_count} times in current run (stall_secs = {stall_secs}s); watchdog Ctrl-C didn't unstick it"
                            ),
                        )
                        .await;
                        action.blocked_notice_posted = true;
                        tracing::warn!(
                            "watchdog: posted blocked_notice for {id} — {stall_count} stalls in current run"
                        );
                    }
                }
            }

            // Prune bookkeeping for agents that no longer exist.
            let alive: std::collections::HashSet<String> = {
                let mgr = manager.lock().await;
                mgr.agent_ids().iter().map(|id| id.0.clone()).collect()
            };
            states.retain(|id, _| alive.contains(id));
        }
    })
}

async fn send_ctrl_c(manager: &Arc<Mutex<SessionManager>>, id: &str) -> Result<(), String> {
    let tx = {
        let mgr = manager.lock().await;
        let session = mgr
            .get_session(&AgentId(id.to_string()))
            .map_err(|_| format!("session {id} not found"))?;
        session.input_sender()
    };
    tx.send(InputEvent::RawBytes { bytes: vec![0x03] })
        .await
        .map_err(|e| e.to_string())
}

/// Write a `blocked_notice` PushEvent to the agent's outbox. The
/// background router picks it up on its next poll (≤ 2 s) and
/// delivers it to the target (coordinator or broadcast).
async fn post_blocked_notice(
    base_dir: &std::path::Path,
    source_agent: &str,
    target_agent: Option<&str>,
    summary: &str,
) {
    let event = PushEvent {
        id: format!(
            "watchdog-{}-{}",
            source_agent,
            chrono::Utc::now().timestamp_millis()
        ),
        kind: PushKind::BlockedNotice,
        source_agent: source_agent.to_string(),
        source_repo: ".".to_string(),
        target_agent: target_agent.map(|s| s.to_string()),
        issue_id: None,
        summary: summary.to_string(),
        wiki_link: None,
        priority: Priority::High,
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    let outbox_dir = base_dir
        .join(ATN_DIR_NAME)
        .join(OUTBOXES_DIR)
        .join(source_agent);
    if tokio::fs::create_dir_all(&outbox_dir).await.is_err() {
        return;
    }
    let file_path = outbox_dir.join(format!("{}.json", event.id));
    if let Ok(json) = serde_json::to_string_pretty(&event) {
        let _ = tokio::fs::write(&file_path, json).await;
    }
}
