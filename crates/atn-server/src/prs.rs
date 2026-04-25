//! `/api/prs` REST surface — list / fetch / merge / reject the
//! `PrRecord` files that `atn-syncd` writes into `<prs-dir>/`.
//!
//! On-disk truth is `<prs-dir>/<id>.json`; the server reads them
//! per-request (no cache) and serializes mutations through a
//! single `tokio::sync::Mutex` so concurrent merge / reject hits
//! can't clobber each other. Writes go via tempfile + rename so a
//! reader never sees a partial JSON.
//!
//! Routes:
//! - `GET    /api/prs` — list, sorted lexically by id; `?status=open`
//!   filter; bad JSON files are skipped with a `tracing::warn!`.
//! - `GET    /api/prs/{id}` — single record or 404.
//! - `POST   /api/prs/{id}/merge` — runs `git merge --no-ff
//!   refs/heads/pr/<agent>-<branch>` on `--central-repo`; on
//!   success flips `status: merged` + `merge_commit` + `merged_at`;
//!   on conflict returns 409 with stderr in the body.
//! - `POST   /api/prs/{id}/reject` — flips `status: rejected` +
//!   `rejected_at`. No git side-effects.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use atn_core::pr::{PrRecord, PrStatus};
use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct PrsState {
    pub prs_dir: PathBuf,
    pub central_repo: PathBuf,
    /// Serializes mutating ops on the prs directory.
    pub lock: Arc<Mutex<()>>,
    /// Fan-out for `/api/prs/stream` subscribers. Mutating
    /// routes push `Updated` events here directly so clients
    /// see the new state even if the filesystem watcher
    /// coalesces the rename event.
    pub broadcast: crate::prs_stream::PrsBroadcast,
}

impl PrsState {
    pub fn new(
        prs_dir: PathBuf,
        central_repo: PathBuf,
        broadcast: crate::prs_stream::PrsBroadcast,
    ) -> Self {
        Self {
            prs_dir,
            central_repo,
            lock: Arc::new(Mutex::new(())),
            broadcast,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    /// Filter by `PrStatus` — `open` / `merged` / `rejected`.
    #[serde(default)]
    pub status: Option<String>,
}

/// Read every `*.json` in `prs_dir` into a `PrRecord`. Anything
/// that fails to parse is logged (`warn!`) and skipped — we'd
/// rather show a partial list than 500 the dashboard.
pub fn read_records(prs_dir: &Path) -> Vec<PrRecord> {
    let mut entries: Vec<PathBuf> = match std::fs::read_dir(prs_dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|s| s.to_str()) == Some("json")
                    && p.is_file()
            })
            .collect(),
        Err(e) => {
            tracing::warn!("prs: read_dir({}) failed: {e}", prs_dir.display());
            return Vec::new();
        }
    };
    entries.sort();
    let mut out = Vec::with_capacity(entries.len());
    for p in entries {
        match std::fs::read_to_string(&p) {
            Ok(s) => match serde_json::from_str::<PrRecord>(&s) {
                Ok(r) => out.push(r),
                Err(e) => tracing::warn!(
                    "prs: skipping {} — parse error: {e}",
                    p.display()
                ),
            },
            Err(e) => tracing::warn!(
                "prs: skipping {} — read error: {e}",
                p.display()
            ),
        }
    }
    out
}

/// Read a single record by id (without the `.json` suffix). The
/// id is path-sandboxed: anything containing `/`, `\\`, `..` or
/// resolving outside `prs_dir` returns `None`.
pub fn read_record(prs_dir: &Path, id: &str) -> Option<PrRecord> {
    let path = sandbox_record_path(prs_dir, id)?;
    let s = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&s).ok()
}

/// Build the on-disk path for a record id, refusing escape attempts.
fn sandbox_record_path(prs_dir: &Path, id: &str) -> Option<PathBuf> {
    if id.is_empty()
        || id.contains('/')
        || id.contains('\\')
        || id.contains("..")
        || id.contains('\0')
    {
        return None;
    }
    Some(prs_dir.join(format!("{id}.json")))
}

/// Atomic write — tempfile + rename so readers never see a
/// half-written record.
pub fn write_record(prs_dir: &Path, record: &PrRecord) -> Result<PathBuf, String> {
    let path = sandbox_record_path(prs_dir, &record.id)
        .ok_or_else(|| format!("invalid record id {:?}", record.id))?;
    let json = serde_json::to_string_pretty(record)
        .map_err(|e| format!("serialize: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("write {:?}: {e}", tmp))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| format!("rename {:?} → {:?}: {e}", tmp, path))?;
    Ok(path)
}

/// Run `git -C <central_repo> <args>`. Returns `(stdout, stderr,
/// exit_code)`. Stdout/stderr are trimmed strings.
fn run_git(central_repo: &Path, args: &[&str]) -> Result<(String, String, i32), String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(central_repo)
        .args(args)
        .output()
        .map_err(|e| format!("git spawn: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let code = out.status.code().unwrap_or(-1);
    Ok((stdout, stderr, code))
}

/// Run `git merge --no-ff refs/heads/pr/<agent>-<branch>` on the
/// central repo. Returns the new HEAD SHA on success; on conflict
/// returns the captured stderr so the caller can surface it as 409.
pub fn git_merge(
    central_repo: &Path,
    agent_id: &str,
    branch: &str,
) -> Result<String, String> {
    let pr_ref = format!("refs/heads/pr/{}-{}", agent_id, branch);
    let msg = format!("Merge {pr_ref}");
    let (stdout, stderr, code) = run_git(
        central_repo,
        &["merge", "--no-ff", "-m", &msg, &pr_ref],
    )?;
    if code != 0 {
        // Bring the worktree back to a clean state so the next
        // attempt isn't blocked by a half-applied merge.
        let _ = run_git(central_repo, &["merge", "--abort"]);
        // git writes conflict markers to stdout and the
        // "Automatic merge failed" notice to stderr; surface both.
        let combined = match (stdout.is_empty(), stderr.is_empty()) {
            (true, _) => stderr,
            (_, true) => stdout,
            _ => format!("{stdout}\n{stderr}"),
        };
        return Err(combined);
    }
    let (sha, _, _) = run_git(central_repo, &["rev-parse", "HEAD"])?;
    Ok(sha)
}

// ── Route handlers ─────────────────────────────────────────────────────

pub async fn list_prs(
    State(state): State<crate::AppState>,
    Query(q): Query<ListQuery>,
) -> Json<Vec<PrRecord>> {
    let prs_dir = state.prs.prs_dir.clone();
    let records = tokio::task::spawn_blocking(move || read_records(&prs_dir))
        .await
        .unwrap_or_default();
    let filtered = match q.status.as_deref() {
        Some("open") => filter_status(records, PrStatus::Open),
        Some("merged") => filter_status(records, PrStatus::Merged),
        Some("rejected") => filter_status(records, PrStatus::Rejected),
        _ => records,
    };
    Json(filtered)
}

fn filter_status(records: Vec<PrRecord>, want: PrStatus) -> Vec<PrRecord> {
    records.into_iter().filter(|r| r.status == want).collect()
}

pub async fn get_pr(
    AxumPath(id): AxumPath<String>,
    State(state): State<crate::AppState>,
) -> Result<Json<PrRecord>, StatusCode> {
    let prs_dir = state.prs.prs_dir.clone();
    let id_clone = id.clone();
    let rec = tokio::task::spawn_blocking(move || read_record(&prs_dir, &id_clone))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(rec))
}

pub async fn merge_pr(
    AxumPath(id): AxumPath<String>,
    State(state): State<crate::AppState>,
) -> Response {
    let _guard = state.prs.lock.lock().await;
    let prs_dir = state.prs.prs_dir.clone();
    let central = state.prs.central_repo.clone();
    let id_clone = id.clone();

    let outcome = tokio::task::spawn_blocking(move || {
        let mut rec = match read_record(&prs_dir, &id_clone) {
            Some(r) => r,
            None => return Err((StatusCode::NOT_FOUND, json!({"error": "pr not found"}))),
        };
        if rec.status != PrStatus::Open {
            return Err((
                StatusCode::CONFLICT,
                json!({
                    "error": "pr is not open",
                    "status": rec.status,
                }),
            ));
        }
        match git_merge(&central, &rec.agent_id, &rec.branch) {
            Ok(merge_sha) => {
                rec.status = PrStatus::Merged;
                rec.merge_commit = Some(merge_sha);
                rec.merged_at = Some(chrono::Utc::now().to_rfc3339());
                rec.last_error = None;
                if let Err(e) = write_record(&prs_dir, &rec) {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        json!({"error": format!("write record: {e}")}),
                    ));
                }
                Ok(rec)
            }
            Err(stderr) => Err((
                StatusCode::CONFLICT,
                json!({
                    "error": "merge failed",
                    "stderr": stderr,
                }),
            )),
        }
    })
    .await;

    match outcome {
        Ok(Ok(rec)) => {
            state
                .prs
                .broadcast
                .send(crate::prs_stream::PrsEvent::Updated { record: rec.clone() });
            Json(rec).into_response()
        }
        Ok(Err((code, body))) => (code, Json(body)).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "merge task panicked"})),
        )
            .into_response(),
    }
}

pub async fn reject_pr(
    AxumPath(id): AxumPath<String>,
    State(state): State<crate::AppState>,
) -> Response {
    let _guard = state.prs.lock.lock().await;
    let prs_dir = state.prs.prs_dir.clone();
    let id_clone = id.clone();

    let outcome = tokio::task::spawn_blocking(move || {
        let mut rec = match read_record(&prs_dir, &id_clone) {
            Some(r) => r,
            None => return Err((StatusCode::NOT_FOUND, json!({"error": "pr not found"}))),
        };
        if rec.status != PrStatus::Open {
            return Err((
                StatusCode::CONFLICT,
                json!({"error": "pr is not open", "status": rec.status}),
            ));
        }
        rec.status = PrStatus::Rejected;
        rec.rejected_at = Some(chrono::Utc::now().to_rfc3339());
        if let Err(e) = write_record(&prs_dir, &rec) {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": format!("write record: {e}")}),
            ));
        }
        Ok(rec)
    })
    .await;

    match outcome {
        Ok(Ok(rec)) => {
            state
                .prs
                .broadcast
                .send(crate::prs_stream::PrsEvent::Updated { record: rec.clone() });
            Json(rec).into_response()
        }
        Ok(Err((code, body))) => (code, Json(body)).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "reject task panicked"})),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sample_record(id: &str) -> PrRecord {
        PrRecord {
            id: id.into(),
            agent_id: "alice".into(),
            source_repo: "/tmp".into(),
            branch: "feature".into(),
            target: "main".into(),
            commit: "abcdef0123".into(),
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
    fn read_records_sorts_lexically_and_skips_garbage() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let r1 = sample_record("alice-feature-aaa1111");
        let r2 = sample_record("alice-feature-bbb2222");
        write_record(dir, &r2).unwrap();
        write_record(dir, &r1).unwrap();
        // Add a garbage file — should be skipped, not crash.
        fs::write(dir.join("garbage.json"), "{not valid json").unwrap();
        // Non-json file is ignored entirely.
        fs::write(dir.join("readme.txt"), "ignore me").unwrap();

        let recs = read_records(dir);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].id, "alice-feature-aaa1111");
        assert_eq!(recs[1].id, "alice-feature-bbb2222");
    }

    #[test]
    fn read_record_round_trip_and_404() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let r = sample_record("alice-feature-cc3");
        write_record(dir, &r).unwrap();
        let back = read_record(dir, "alice-feature-cc3").unwrap();
        assert_eq!(back, r);
        assert!(read_record(dir, "missing-id").is_none());
    }

    #[test]
    fn read_record_rejects_path_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        for bad in ["", "..", "../etc", "a/b", "a\\b", "a\0b"] {
            assert!(
                read_record(tmp.path(), bad).is_none(),
                "expected None for id {bad:?}"
            );
        }
    }

    #[test]
    fn write_record_is_atomic_no_tmp_left() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let r = sample_record("alice-feature-zz9");
        write_record(dir, &r).unwrap();
        let entries: Vec<_> = fs::read_dir(dir).unwrap().collect();
        assert_eq!(entries.len(), 1);
        let n = entries[0].as_ref().unwrap().file_name();
        assert_eq!(n, "alice-feature-zz9.json");
    }

    #[test]
    fn filter_status_keeps_only_matching() {
        let mut a = sample_record("a");
        a.status = PrStatus::Open;
        let mut b = sample_record("b");
        b.status = PrStatus::Merged;
        let mut c = sample_record("c");
        c.status = PrStatus::Rejected;
        let only_open = filter_status(vec![a.clone(), b, c], PrStatus::Open);
        assert_eq!(only_open.len(), 1);
        assert_eq!(only_open[0].id, "a");
    }

    #[test]
    fn read_records_empty_dir_is_empty_vec() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_records(tmp.path()).is_empty());
    }

    #[test]
    fn read_records_missing_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist");
        assert!(read_records(&missing).is_empty());
    }

    /// Smoke test for `git_merge` on a real local fixture. Doesn't
    /// exercise the route — those go through the integration test.
    #[test]
    fn git_merge_conflict_returns_stderr() {
        let tmp = tempfile::tempdir().unwrap();
        let central = tmp.path().join("central");
        let work = tmp.path().join("work");

        fn run(p: &Path, args: &[&str]) {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(p)
                .args(args)
                .output()
                .expect("git spawn");
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }

        // Set up a non-bare central with one commit on main.
        std::fs::create_dir_all(&central).unwrap();
        std::process::Command::new("git")
            .args(["init", "--initial-branch=main"])
            .arg(&central)
            .output()
            .unwrap();
        run(&central, &["config", "user.email", "t@t"]);
        run(&central, &["config", "user.name", "T"]);
        run(&central, &["config", "commit.gpgsign", "false"]);
        run(
            &central,
            &["config", "receive.denyCurrentBranch", "ignore"],
        );
        std::fs::write(central.join("a.txt"), "central\n").unwrap();
        run(&central, &["add", "a.txt"]);
        run(&central, &["commit", "-m", "init"]);

        // Clone into worktree, branch off, conflict on a.txt, push.
        std::process::Command::new("git")
            .args(["clone"])
            .arg(&central)
            .arg(&work)
            .output()
            .unwrap();
        run(&work, &["config", "user.email", "t@t"]);
        run(&work, &["config", "user.name", "T"]);
        run(&work, &["config", "commit.gpgsign", "false"]);
        run(&work, &["checkout", "-b", "feature"]);
        std::fs::write(work.join("a.txt"), "worktree\n").unwrap();
        run(&work, &["add", "a.txt"]);
        run(&work, &["commit", "-m", "wt"]);
        run(
            &work,
            &["push", "origin", "feature:refs/heads/pr/alice-feature"],
        );

        // Make central main diverge.
        std::fs::write(central.join("a.txt"), "different\n").unwrap();
        run(&central, &["add", "a.txt"]);
        run(&central, &["commit", "-m", "diverge"]);

        let err = git_merge(&central, "alice", "feature").unwrap_err();
        assert!(
            err.to_lowercase().contains("conflict") || err.contains("Automatic merge failed"),
            "expected conflict in stderr, got {err:?}"
        );
        // After abort, central is back to a clean state.
        let (head, _, _) = run_git(&central, &["status", "--short"]).unwrap();
        assert!(head.is_empty(), "expected clean status, got {head:?}");
    }
}
