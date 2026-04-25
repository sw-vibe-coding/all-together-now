//! `/api/prs/stream` SSE endpoint + filesystem watcher.
//!
//! The dashboard PR panel subscribes to this stream so it can
//! render registry deltas live without polling. On connect the
//! server replies with a `Snapshot { records }` summarizing
//! everything that's currently in `<prs-dir>`; subsequent events
//! arrive as `Created { record }`, `Updated { record }`, or
//! `Removed { id }` as the directory changes.
//!
//! The watcher coalesces filesystem events within ~50 ms so a
//! `tempfile + rename` write doesn't fan out twice. Mutating
//! routes (`merge` / `reject`) ALSO push `Updated` events on the
//! broadcast directly, because some platforms collapse rename
//! events into a single `Modify` and the client should converge
//! to the latest record either way (it de-dupes by id + status).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use atn_core::pr::PrRecord;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::prs::read_records;

/// Capacity for the broadcast channel — generous so a slow
/// dashboard tab doesn't drop intermediate updates.
const BROADCAST_CAPACITY: usize = 256;

/// Coalescing window: drain bursts of notify events that fire
/// within this duration before re-reading + emitting.
const COALESCE_WINDOW: Duration = Duration::from_millis(50);

/// Snake-case discriminated union the dashboard can match on.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum PrsEvent {
    /// First payload sent on a fresh `/api/prs/stream` connection.
    Snapshot { records: Vec<PrRecord> },
    /// A new `<prs-dir>/<id>.json` appeared.
    Created { record: PrRecord },
    /// An existing record's body changed (typically a status
    /// flip from a merge / reject route).
    Updated { record: PrRecord },
    /// `<prs-dir>/<id>.json` was removed.
    Removed { id: String },
}

/// Sender half of the in-process broadcast. Cheap to clone.
#[derive(Clone)]
pub struct PrsBroadcast {
    sender: broadcast::Sender<PrsEvent>,
}

impl PrsBroadcast {
    pub fn new() -> Self {
        let (sender, _rx) = broadcast::channel(BROADCAST_CAPACITY);
        Self { sender }
    }

    /// Best-effort fan-out. If no clients are connected the send
    /// returns `Err` from `tokio::sync::broadcast`; we swallow
    /// it because mutating routes still need to succeed.
    pub fn send(&self, ev: PrsEvent) {
        let _ = self.sender.send(ev);
    }

    /// New subscriber. Existing buffered events (up to
    /// `BROADCAST_CAPACITY`) are replayed.
    pub fn subscribe(&self) -> broadcast::Receiver<PrsEvent> {
        self.sender.subscribe()
    }
}

impl Default for PrsBroadcast {
    fn default() -> Self {
        Self::new()
    }
}

/// Run the watcher on a dedicated OS thread (notify is sync) +
/// a small async loop that coalesces and translates events.
pub fn spawn_watcher(prs_dir: PathBuf, broadcast: PrsBroadcast) {
    let (raw_tx, raw_rx) = std::sync::mpsc::channel::<notify::Event>();

    std::thread::spawn(move || {
        use notify::{RecursiveMode, Watcher};
        let watcher = notify::recommended_watcher(
            move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = raw_tx.send(event);
                }
            },
        );
        let mut watcher = match watcher {
            Ok(w) => w,
            Err(e) => {
                tracing::error!("prs watcher: failed to create: {e}");
                return;
            }
        };
        if let Err(e) = watcher.watch(&prs_dir, RecursiveMode::NonRecursive) {
            tracing::error!(
                "prs watcher: failed to watch {}: {e}",
                prs_dir.display()
            );
            return;
        }
        tracing::info!("prs watcher: watching {}", prs_dir.display());

        // Track ids we've already emitted Created for so subsequent
        // modifies map to Updated. Persists for the watcher's
        // lifetime; ids stick around even after Removed (the
        // dashboard de-dupes by id+status, so worst case is one
        // Updated where Created might have been more accurate —
        // not a correctness issue).
        let seen: Arc<std::sync::Mutex<HashSet<String>>> = Arc::new(std::sync::Mutex::new({
            let mut s = HashSet::new();
            for r in read_records(&prs_dir) {
                s.insert(r.id.clone());
            }
            s
        }));
        let prs_dir_for_loop = prs_dir.clone();

        loop {
            let first = match raw_rx.recv() {
                Ok(e) => e,
                Err(_) => {
                    tracing::warn!("prs watcher: notify channel closed, exiting");
                    return;
                }
            };
            let mut events = vec![first];
            let deadline = Instant::now() + COALESCE_WINDOW;
            while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
                if remaining.is_zero() {
                    break;
                }
                match raw_rx.recv_timeout(remaining) {
                    Ok(e) => events.push(e),
                    Err(_) => break,
                }
            }
            translate_and_broadcast(
                &prs_dir_for_loop,
                events,
                &broadcast,
                Arc::clone(&seen),
            );
        }
    });
}

/// Map a coalesced burst of notify events to `PrsEvent`s on the
/// broadcast. Pure aside from filesystem reads and the broadcast
/// send; testable by passing in synthesized events.
fn translate_and_broadcast(
    prs_dir: &Path,
    events: Vec<notify::Event>,
    broadcast: &PrsBroadcast,
    seen: Arc<std::sync::Mutex<HashSet<String>>>,
) {
    use notify::EventKind;
    let mut touched: HashSet<PathBuf> = HashSet::new();
    let mut removed: HashSet<PathBuf> = HashSet::new();
    for ev in events {
        for p in &ev.paths {
            if p.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            match ev.kind {
                EventKind::Remove(_) => {
                    removed.insert(p.clone());
                    touched.remove(p);
                }
                _ => {
                    touched.insert(p.clone());
                    removed.remove(p);
                }
            }
        }
    }

    for path in touched {
        // Filename → id (strip `.json`). Skip files that don't
        // sit directly under the watched dir.
        let id = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        if path.parent() != Some(prs_dir) {
            continue;
        }
        let body = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                // Common race: rename in-flight, file vanished
                // between notify and our read. Ignore and the
                // next event will catch up.
                tracing::debug!("prs watcher: read {}: {e}", path.display());
                continue;
            }
        };
        let record: PrRecord = match serde_json::from_str(&body) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("prs watcher: skipping {}: {e}", path.display());
                continue;
            }
        };
        let is_new = {
            let mut s = match seen.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            s.insert(id.clone())
        };
        if is_new {
            broadcast.send(PrsEvent::Created { record });
        } else {
            broadcast.send(PrsEvent::Updated { record });
        }
    }
    for path in removed {
        let id = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let was_known = {
            let mut s = match seen.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            s.remove(&id)
        };
        if was_known {
            broadcast.send(PrsEvent::Removed { id });
        }
    }
}

/// `GET /api/prs/stream` — sends a `Snapshot` first, then live
/// deltas from the broadcast. Server-side keep-alive every 15 s
/// so transparent proxies don't reap idle connections.
pub async fn pr_stream(
    State(state): State<crate::AppState>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let prs_dir = state.prs.prs_dir.clone();
    let mut rx = state.prs.broadcast.subscribe();

    let (tx, sse_rx) = tokio::sync::mpsc::channel::<Event>(64);

    tokio::spawn(async move {
        // Snapshot first.
        let records = tokio::task::spawn_blocking(move || read_records(&prs_dir))
            .await
            .unwrap_or_default();
        let snapshot = PrsEvent::Snapshot { records };
        if tx.send(event_for(&snapshot)).await.is_err() {
            return;
        }
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if tx.send(event_for(&ev)).await.is_err() {
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    // Slow client missed n events; keep going.
                    tracing::warn!("prs stream: lagging client missed {n} events");
                }
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(sse_rx)
        .map(Ok::<_, std::convert::Infallible>);
    Sse::new(stream).keep_alive(KeepAlive::default().interval(Duration::from_secs(15)))
}

fn event_for(ev: &PrsEvent) -> Event {
    let json = serde_json::to_string(ev).unwrap_or_else(|_| "{}".to_string());
    Event::default().data(json)
}

// Re-export StreamExt::map without importing it on every call site.
use tokio_stream::StreamExt as _;

#[cfg(test)]
mod tests {
    use super::*;
    use atn_core::pr::PrStatus;

    fn sample(id: &str) -> PrRecord {
        PrRecord {
            id: id.into(),
            agent_id: "alice".into(),
            source_repo: "/tmp".into(),
            branch: "feature".into(),
            target: "main".into(),
            commit: "abcdef0".into(),
            summary: "test".into(),
            status: PrStatus::Open,
            created_at: "2026-04-25T00:00:00Z".into(),
            merge_commit: None,
            merged_at: None,
            rejected_at: None,
            last_error: None,
        }
    }

    #[test]
    fn snapshot_serializes_with_event_tag() {
        let ev = PrsEvent::Snapshot {
            records: vec![sample("alice-feature-aaa1111")],
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"event\":\"snapshot\""));
        assert!(json.contains("\"records\""));
        assert!(json.contains("\"alice-feature-aaa1111\""));
    }

    #[test]
    fn created_serializes_with_event_tag() {
        let ev = PrsEvent::Created { record: sample("a") };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"event\":\"created\""));
        assert!(json.contains("\"record\""));
    }

    #[test]
    fn updated_serializes_with_event_tag() {
        let ev = PrsEvent::Updated { record: sample("a") };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"event\":\"updated\""));
    }

    #[test]
    fn removed_serializes_with_event_tag_and_id() {
        let ev = PrsEvent::Removed { id: "alice-feature-bbb".into() };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"event\":\"removed\""));
        assert!(json.contains("\"id\":\"alice-feature-bbb\""));
    }

    #[test]
    fn prs_event_round_trips_through_serde() {
        let ev = PrsEvent::Updated { record: sample("alice-feature-zz9") };
        let json = serde_json::to_string(&ev).unwrap();
        let back: PrsEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn broadcast_send_with_no_subscribers_is_silent() {
        let bc = PrsBroadcast::new();
        // No subscribers; this MUST NOT panic or surface an error
        // to the caller (mutating routes still need to succeed).
        bc.send(PrsEvent::Removed { id: "x".into() });
    }

    #[test]
    fn broadcast_subscriber_receives_send() {
        let bc = PrsBroadcast::new();
        let mut rx = bc.subscribe();
        bc.send(PrsEvent::Removed { id: "x".into() });
        let got = rx.try_recv().unwrap();
        assert!(matches!(&got, PrsEvent::Removed { id } if id == "x"));
    }

    /// `translate_and_broadcast` is the heart of the watcher; the
    /// notify::Event constructor takes paths + a kind so we can
    /// drive it with synthesized events.
    fn synth_event(kind: notify::EventKind, paths: Vec<PathBuf>) -> notify::Event {
        notify::Event {
            kind,
            paths,
            attrs: Default::default(),
        }
    }

    #[test]
    fn translate_emits_created_then_updated_for_same_id() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("alice-feature-zz9.json");
        let record = sample("alice-feature-zz9");
        std::fs::write(&path, serde_json::to_string(&record).unwrap()).unwrap();

        let bc = PrsBroadcast::new();
        let mut rx = bc.subscribe();
        let seen: Arc<std::sync::Mutex<HashSet<String>>> =
            Arc::new(std::sync::Mutex::new(HashSet::new()));

        // First write → Created.
        translate_and_broadcast(
            tmp.path(),
            vec![synth_event(
                notify::EventKind::Create(notify::event::CreateKind::File),
                vec![path.clone()],
            )],
            &bc,
            Arc::clone(&seen),
        );
        let first = rx.try_recv().unwrap();
        assert!(matches!(first, PrsEvent::Created { .. }), "got {first:?}");

        // Second touch → Updated (same id).
        translate_and_broadcast(
            tmp.path(),
            vec![synth_event(
                notify::EventKind::Modify(notify::event::ModifyKind::Data(
                    notify::event::DataChange::Content,
                )),
                vec![path.clone()],
            )],
            &bc,
            Arc::clone(&seen),
        );
        let second = rx.try_recv().unwrap();
        assert!(matches!(second, PrsEvent::Updated { .. }), "got {second:?}");
    }

    #[test]
    fn translate_emits_removed_when_known_id_disappears() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("alice-feature-zz9.json");
        // No file on disk — it was removed.

        let bc = PrsBroadcast::new();
        let mut rx = bc.subscribe();
        let seen: Arc<std::sync::Mutex<HashSet<String>>> =
            Arc::new(std::sync::Mutex::new({
                let mut s = HashSet::new();
                s.insert("alice-feature-zz9".to_string());
                s
            }));

        translate_and_broadcast(
            tmp.path(),
            vec![synth_event(
                notify::EventKind::Remove(notify::event::RemoveKind::File),
                vec![path],
            )],
            &bc,
            Arc::clone(&seen),
        );
        let got = rx.try_recv().unwrap();
        assert!(
            matches!(&got, PrsEvent::Removed { id } if id == "alice-feature-zz9"),
            "got {got:?}"
        );
    }

    #[test]
    fn translate_skips_non_json_files() {
        let tmp = tempfile::tempdir().unwrap();
        let bc = PrsBroadcast::new();
        let mut rx = bc.subscribe();
        let seen = Arc::new(std::sync::Mutex::new(HashSet::new()));

        translate_and_broadcast(
            tmp.path(),
            vec![synth_event(
                notify::EventKind::Create(notify::event::CreateKind::File),
                vec![tmp.path().join("readme.txt")],
            )],
            &bc,
            seen,
        );
        assert!(rx.try_recv().is_err(), "unexpected event for non-json");
    }

    #[test]
    fn translate_dedupes_create_and_remove_in_same_burst() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("alice-feature-zz9.json");
        let bc = PrsBroadcast::new();
        let mut rx = bc.subscribe();
        let seen = Arc::new(std::sync::Mutex::new(HashSet::new()));

        // Create + remove in one burst → only Remove should land
        // (the file isn't there to read for the Create path).
        translate_and_broadcast(
            tmp.path(),
            vec![
                synth_event(
                    notify::EventKind::Create(notify::event::CreateKind::File),
                    vec![path.clone()],
                ),
                synth_event(
                    notify::EventKind::Remove(notify::event::RemoveKind::File),
                    vec![path.clone()],
                ),
            ],
            &bc,
            seen,
        );
        // No Created (file doesn't exist), and no Removed (we
        // never knew about that id), so the channel stays empty.
        assert!(rx.try_recv().is_err());
    }
}
