//! Client-side debug log endpoint.
//!
//! The dashboard JS posts entries here so we can introspect what the
//! browser actually did (renders, errors, focus changes) without
//! needing playwright. Stored in a bounded in-memory ring + appended
//! to `.atn/client-log.jsonl` on disk for post-mortem inspection.
//!
//! Entries flow:
//!   POST /api/client-log  (browser)        → ring + jsonl
//!   GET  /api/client-log?since=N           ← verify.sh / cli
//!
//! Server stamps `seq` (monotonically increasing per session) and
//! `received_at` (RFC3339). The browser supplies `level`, `source`,
//! optional `agent_id`, `message`, `data`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::collections::VecDeque;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

const RING_CAPACITY: usize = 2000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientLogEntry {
    pub seq: u64,
    pub received_at: String,
    pub level: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ClientLogPostBody {
    pub level: String,
    pub source: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    pub message: String,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ClientLogQuery {
    #[serde(default)]
    pub since: Option<u64>,
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
}

#[derive(Clone)]
pub struct ClientLogState {
    inner: Arc<Mutex<ClientLogInner>>,
    file_path: PathBuf,
}

struct ClientLogInner {
    next_seq: u64,
    ring: VecDeque<ClientLogEntry>,
}

impl ClientLogState {
    pub fn new(base_dir: &std::path::Path) -> Self {
        let file_path = base_dir.join(".atn").join("client-log.jsonl");
        if let Some(parent) = file_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::warn!(
                    "client-log: create_dir_all({}) failed: {e}",
                    parent.display()
                );
            }
        }
        Self {
            inner: Arc::new(Mutex::new(ClientLogInner {
                next_seq: 0,
                ring: VecDeque::with_capacity(RING_CAPACITY),
            })),
            file_path,
        }
    }

    fn append(&self, body: ClientLogPostBody) -> ClientLogEntry {
        let mut inner = self.inner.lock().expect("client-log mutex poisoned");
        let seq = inner.next_seq;
        inner.next_seq += 1;
        let entry = ClientLogEntry {
            seq,
            received_at: chrono::Utc::now().to_rfc3339(),
            level: body.level,
            source: body.source,
            agent_id: body.agent_id,
            message: body.message,
            data: body.data,
        };
        if inner.ring.len() == RING_CAPACITY {
            inner.ring.pop_front();
        }
        inner.ring.push_back(entry.clone());
        // Append to disk. Failures don't poison the in-memory ring but
        // every error path surfaces through tracing so we can diagnose
        // without re-reading the source.
        match serde_json::to_string(&entry) {
            Err(e) => tracing::warn!("client-log: serialize entry seq={seq} failed: {e}"),
            Ok(json) => match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.file_path)
            {
                Err(e) => tracing::warn!(
                    "client-log: open {} for append failed: {e}",
                    self.file_path.display()
                ),
                Ok(mut f) => {
                    use std::io::Write;
                    if let Err(e) = writeln!(f, "{json}") {
                        tracing::warn!(
                            "client-log: write entry seq={seq} to {} failed: {e}",
                            self.file_path.display()
                        );
                    }
                }
            },
        }
        entry
    }

    fn snapshot(&self, q: &ClientLogQuery) -> Vec<ClientLogEntry> {
        let inner = self.inner.lock().expect("client-log mutex poisoned");
        inner
            .ring
            .iter()
            .filter(|e| q.since.is_none_or(|s| e.seq >= s))
            .filter(|e| q.level.as_deref().is_none_or(|l| e.level == l))
            .filter(|e| q.source.as_deref().is_none_or(|s| e.source == s))
            .filter(|e| {
                q.agent_id
                    .as_deref()
                    .is_none_or(|a| e.agent_id.as_deref() == Some(a))
            })
            .cloned()
            .collect()
    }
}

pub async fn submit_client_log(
    State(state): State<crate::AppState>,
    Json(body): Json<ClientLogPostBody>,
) -> (StatusCode, Json<ClientLogEntry>) {
    let entry = state.client_log.append(body);
    (StatusCode::ACCEPTED, Json(entry))
}

pub async fn list_client_log(
    State(state): State<crate::AppState>,
    Query(q): Query<ClientLogQuery>,
) -> Json<Vec<ClientLogEntry>> {
    Json(state.client_log.snapshot(&q))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_post(level: &str, source: &str, msg: &str) -> ClientLogPostBody {
        ClientLogPostBody {
            level: level.to_string(),
            source: source.to_string(),
            agent_id: None,
            message: msg.to_string(),
            data: None,
        }
    }

    #[test]
    fn append_and_snapshot_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ClientLogState::new(tmp.path());
        let e1 = s.append(fake_post("info", "boot", "page loaded"));
        let e2 = s.append(fake_post("error", "graphRefresh", "fetch failed"));
        assert_eq!(e1.seq, 0);
        assert_eq!(e2.seq, 1);
        let all = s.snapshot(&ClientLogQuery {
            since: None,
            level: None,
            source: None,
            agent_id: None,
        });
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].message, "page loaded");
        assert_eq!(all[1].source, "graphRefresh");
    }

    #[test]
    fn ring_caps_at_capacity() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ClientLogState::new(tmp.path());
        for i in 0..(RING_CAPACITY + 50) {
            s.append(fake_post("info", "tick", &format!("entry {i}")));
        }
        let all = s.snapshot(&ClientLogQuery {
            since: None,
            level: None,
            source: None,
            agent_id: None,
        });
        assert_eq!(all.len(), RING_CAPACITY);
        // Oldest 50 dropped; first remaining seq = 50.
        assert_eq!(all.first().unwrap().seq, 50);
        assert_eq!(all.last().unwrap().seq, (RING_CAPACITY + 50 - 1) as u64);
    }

    #[test]
    fn since_filter_returns_only_recent() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ClientLogState::new(tmp.path());
        for i in 0..10 {
            s.append(fake_post("info", "x", &format!("e{i}")));
        }
        let recent = s.snapshot(&ClientLogQuery {
            since: Some(7),
            level: None,
            source: None,
            agent_id: None,
        });
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].seq, 7);
    }

    #[test]
    fn level_and_source_filters_compose() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ClientLogState::new(tmp.path());
        s.append(fake_post("info", "boot", "ok"));
        s.append(fake_post("error", "boot", "bad"));
        s.append(fake_post("error", "tick", "tickbad"));
        let errs_in_boot = s.snapshot(&ClientLogQuery {
            since: None,
            level: Some("error".to_string()),
            source: Some("boot".to_string()),
            agent_id: None,
        });
        assert_eq!(errs_in_boot.len(), 1);
        assert_eq!(errs_in_boot[0].message, "bad");
    }

    #[test]
    fn jsonl_appended_to_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ClientLogState::new(tmp.path());
        s.append(fake_post("info", "boot", "first"));
        s.append(fake_post("warn", "boot", "second"));
        let path = tmp.path().join(".atn").join("client-log.jsonl");
        let body = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"first\""));
        assert!(lines[1].contains("\"second\""));
    }
}
