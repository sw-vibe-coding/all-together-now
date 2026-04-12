use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::Mutex;

use atn_core::event::{InputEvent, PushEvent};
use atn_core::inbox::{ATN_DIR_NAME, INBOXES_DIR, InboxMessage, OUTBOXES_DIR};
use atn_core::router::{EventLogEntry, RouteDecision, route_event};
use atn_pty::manager::SessionManager;
use atn_wiki::coordination;
use atn_wiki::storage::FileWikiStorage;
use wiki_common::async_storage::AsyncWikiStorage;

/// Shared, append-only event log accessible from REST endpoints.
pub type EventLog = Arc<Mutex<Vec<EventLogEntry>>>;

/// Create the outbox directories for all configured agents.
pub async fn ensure_outbox_dirs(base_dir: &Path, agent_ids: &[String]) {
    let outbox_root = base_dir.join(ATN_DIR_NAME).join(OUTBOXES_DIR);
    let inbox_root = base_dir.join(ATN_DIR_NAME).join(INBOXES_DIR);
    for id in agent_ids {
        let _ = tokio::fs::create_dir_all(outbox_root.join(id)).await;
        let _ = tokio::fs::create_dir_all(inbox_root.join(id)).await;
    }
}

/// Spawn the background message router task.
///
/// Polls agent outbox directories every `poll_interval` for new `.json` files.
/// Routes each event according to `route_event`, delivers to target PTY or
/// escalates to wiki, and appends to the shared event log.
pub fn spawn_message_router(
    base_dir: PathBuf,
    manager: Arc<Mutex<SessionManager>>,
    wiki: Arc<FileWikiStorage>,
    event_log: EventLog,
    poll_interval: std::time::Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let outbox_root = base_dir.join(ATN_DIR_NAME).join(OUTBOXES_DIR);
        let inbox_root = base_dir.join(ATN_DIR_NAME).join(INBOXES_DIR);

        loop {
            if let Err(e) = poll_once(&outbox_root, &inbox_root, &manager, &wiki, &event_log).await
            {
                tracing::warn!("Router poll error: {e}");
            }
            tokio::time::sleep(poll_interval).await;
        }
    })
}

async fn poll_once(
    outbox_root: &Path,
    inbox_root: &Path,
    manager: &Arc<Mutex<SessionManager>>,
    wiki: &Arc<FileWikiStorage>,
    event_log: &EventLog,
) -> Result<(), String> {
    // List agent outbox directories.
    let mut dir_entries = tokio::fs::read_dir(outbox_root)
        .await
        .map_err(|e| format!("read outbox root: {e}"))?;

    while let Some(entry) = dir_entries
        .next_entry()
        .await
        .map_err(|e| format!("read entry: {e}"))?
    {
        let agent_dir = entry.path();
        if !agent_dir.is_dir() {
            continue;
        }

        // Scan for .json files in this agent's outbox.
        let mut files = tokio::fs::read_dir(&agent_dir)
            .await
            .map_err(|e| format!("read agent outbox: {e}"))?;

        while let Some(file_entry) = files
            .next_entry()
            .await
            .map_err(|e| format!("read file: {e}"))?
        {
            let path = file_entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                match process_outbox_file(&path, inbox_root, manager, wiki, event_log).await {
                    Ok(()) => {
                        // Move processed file to .done to avoid reprocessing.
                        let done_path = path.with_extension("json.done");
                        let _ = tokio::fs::rename(&path, &done_path).await;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to process {}: {e}", path.display());
                    }
                }
            }
        }
    }
    Ok(())
}

async fn process_outbox_file(
    path: &Path,
    inbox_root: &Path,
    manager: &Arc<Mutex<SessionManager>>,
    wiki: &Arc<FileWikiStorage>,
    event_log: &EventLog,
) -> Result<(), String> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("read file: {e}"))?;
    let event: PushEvent =
        serde_json::from_str(&content).map_err(|e| format!("parse PushEvent: {e}"))?;

    tracing::info!(
        "Router: event {} from {} → {:?}",
        event.id,
        event.source_agent,
        event.target_agent
    );

    // Get known agent IDs.
    let known_agents: HashSet<String> = {
        let mgr = manager.lock().await;
        mgr.agent_ids().iter().map(|id| id.0.clone()).collect()
    };

    let decision = route_event(&event, &known_agents);
    let now = chrono::Utc::now().to_rfc3339();
    let mut delivered = false;

    match &decision {
        RouteDecision::DeliverToAgent { agent_id } => {
            // Write to target agent's inbox.
            let inbox_dir = inbox_root.join(agent_id);
            let _ = tokio::fs::create_dir_all(&inbox_dir).await;
            let inbox_msg = InboxMessage {
                event: event.clone(),
                delivered: true,
                delivered_at: Some(now.clone()),
            };
            let inbox_path = inbox_dir.join(format!("{}.json", event.id));
            let json = serde_json::to_string_pretty(&inbox_msg)
                .map_err(|e| format!("serialize inbox: {e}"))?;
            tokio::fs::write(&inbox_path, json)
                .await
                .map_err(|e| format!("write inbox: {e}"))?;

            // Deliver the event as a prompt to the target agent's TUI.
            // The summary becomes the agent's next instruction; detail goes
            // to the inbox JSON file (already written above) for context.
            let prompt = if let Some(ref detail) = event.wiki_link {
                format!(
                    "[ATN task from {}] {}\n\nSee: {}",
                    event.source_agent, event.summary, detail
                )
            } else {
                format!("[ATN task from {}] {}", event.source_agent, event.summary)
            };
            let tx = {
                let mgr = manager.lock().await;
                mgr.get_session(&atn_core::agent::AgentId(agent_id.clone()))
                    .ok()
                    .map(|s| s.input_sender())
            };
            if let Some(tx) = tx {
                let _ = tx
                    .send(InputEvent::HumanText { text: prompt })
                    .await;
            }
            delivered = true;
            tracing::info!("Router: delivered event {} to agent {}", event.id, agent_id);
        }
        RouteDecision::Escalate { reason } => {
            // Write to wiki Requests page and log for human attention.
            let entry = format!(
                "**[ESCALATION]** {} → unknown target. Source: {}, Kind: {:?}. Reason: {}",
                event.summary, event.source_agent, event.kind, reason,
            );
            let ts = &now;
            let wiki_now = wiki_common::time::now();
            coordination::append_log(wiki.as_ref(), &entry, ts, wiki_now).await;
            append_to_requests(wiki.as_ref(), &event, reason, wiki_now).await;
            tracing::warn!("Router: escalated event {} — {}", event.id, reason);
        }
        RouteDecision::Broadcast => {
            // Write to wiki Requests page for visibility.
            let entry = format!(
                "**[BROADCAST]** {} (from {}, kind: {:?})",
                event.summary, event.source_agent, event.kind,
            );
            let ts = &now;
            let wiki_now = wiki_common::time::now();
            coordination::append_log(wiki.as_ref(), &entry, ts, wiki_now).await;
            append_to_requests(wiki.as_ref(), &event, "no target specified", wiki_now).await;
            tracing::info!("Router: broadcast event {} to wiki", event.id);
        }
    }

    // Append to shared event log.
    let log_entry = EventLogEntry {
        event,
        decision: decision.to_string(),
        delivered,
        logged_at: now,
    };
    event_log.lock().await.push(log_entry);

    Ok(())
}

async fn append_to_requests(wiki: &dyn AsyncWikiStorage, event: &PushEvent, note: &str, now: u64) {
    let title = coordination::REQUESTS_PAGE;
    let mut page = wiki
        .get_page(title)
        .await
        .unwrap_or_else(|| wiki_common::model::WikiPage::new(title, "# Requests\n\n", now));

    page.content.push_str(&format!(
        "\n## {} [{}]\n- **From:** {}\n- **Kind:** {:?}\n- **Priority:** {:?}\n- **Note:** {}\n- **ID:** {}\n",
        event.summary,
        event.timestamp,
        event.source_agent,
        event.kind,
        event.priority,
        note,
        event.id,
    ));
    page.updated_at = now;
    wiki.save_page(page).await;
}
