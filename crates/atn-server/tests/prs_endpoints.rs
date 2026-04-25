//! Integration tests for the `/api/prs` REST surface (git-sync-agents
//! saga step 3).
//!
//! Builds a tempdir with a non-bare central repo, a clone (the
//! "worktree"), pushes two PR refs (`pr/alice-feature` and
//! `pr/alice-feature-z`), drops two pre-canned `PrRecord` JSON
//! files into a `prs-dir`, then boots `atn-server` with the new
//! `--prs-dir` and `--central-repo` flags. Exercises every route:
//!
//! - `GET  /api/prs`            → list (filtered + unfiltered)
//! - `GET  /api/prs/{id}`       → 200 + 404
//! - `POST /api/prs/{id}/merge` → status flips to merged + central
//!   main has the new commit
//! - `POST /api/prs/{id}/reject`→ status flips to rejected

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn server_binary() -> PathBuf {
    let root = repo_root();
    let bin = root.join("target").join("debug").join("atn-server");
    if !bin.exists() {
        let status = Command::new(env!("CARGO"))
            .args(["build", "-p", "atn-server"])
            .current_dir(&root)
            .status()
            .expect("cargo build -p atn-server failed to run");
        assert!(status.success(), "cargo build -p atn-server failed");
    }
    bin
}

fn run_git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("git spawn");
    assert!(
        out.status.success(),
        "git -C {} {:?} failed: {}",
        repo.display(),
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn run_git_capture(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("git spawn");
    assert!(
        out.status.success(),
        "git -C {} {:?} failed: {}",
        repo.display(),
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

struct Fixture {
    _tmp: tempfile::TempDir,
    base_dir: PathBuf,
    central: PathBuf,
    prs_dir: PathBuf,
    /// Open PR id → branch (for assertions).
    feature_id: String,
    feature_z_id: String,
}

fn build_fixture() -> Fixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base_dir = tmp.path().to_path_buf();
    let central = base_dir.join("central");
    let work = base_dir.join("work");
    let prs_dir = base_dir.join(".atn").join("prs");
    std::fs::create_dir_all(&prs_dir).unwrap();

    // Need an agents.toml for atn-server's positional config arg.
    std::fs::write(
        base_dir.join("agents.toml"),
        "[project]\nname = \"prs-test\"\nlog_dir = \".atn/logs\"\n",
    )
    .unwrap();

    // Central: non-bare, main + commit. denyCurrentBranch=ignore so
    // tooling can also push to main if it ever wants to (we don't
    // here, but keep it permissive for end-to-end smoke).
    Command::new("git")
        .args(["init", "--initial-branch=main"])
        .arg(&central)
        .output()
        .unwrap();
    run_git(&central, &["config", "user.email", "central@test"]);
    run_git(&central, &["config", "user.name", "Central"]);
    run_git(&central, &["config", "commit.gpgsign", "false"]);
    run_git(&central, &["config", "receive.denyCurrentBranch", "ignore"]);
    std::fs::write(central.join("README.md"), "central\n").unwrap();
    run_git(&central, &["add", "README.md"]);
    run_git(&central, &["commit", "-m", "init"]);

    // Worktree (clone of central), shares history.
    Command::new("git")
        .args(["clone"])
        .arg(&central)
        .arg(&work)
        .output()
        .unwrap();
    run_git(&work, &["config", "user.email", "agent@test"]);
    run_git(&work, &["config", "user.name", "Agent"]);
    run_git(&work, &["config", "commit.gpgsign", "false"]);

    // PR #1: feature → modifies a new file (no conflict with main).
    run_git(&work, &["checkout", "-b", "feature"]);
    std::fs::write(work.join("feature.txt"), "feature\n").unwrap();
    run_git(&work, &["add", "feature.txt"]);
    run_git(&work, &["commit", "-m", "add feature"]);
    run_git(
        &work,
        &["push", "origin", "feature:refs/heads/pr/alice-feature"],
    );
    let feature_sha = run_git_capture(&work, &["rev-parse", "feature"]);
    let feature_short: String = feature_sha.chars().take(7).collect();
    let feature_id = format!("alice-feature-{feature_short}");

    // PR #2: feature-z (will be rejected, not merged).
    run_git(&work, &["checkout", "main"]);
    run_git(&work, &["checkout", "-b", "feature-z"]);
    std::fs::write(work.join("z.txt"), "z\n").unwrap();
    run_git(&work, &["add", "z.txt"]);
    run_git(&work, &["commit", "-m", "add z"]);
    run_git(
        &work,
        &["push", "origin", "feature-z:refs/heads/pr/alice-feature-z"],
    );
    let z_sha = run_git_capture(&work, &["rev-parse", "feature-z"]);
    let z_short: String = z_sha.chars().take(7).collect();
    let feature_z_id = format!("alice-feature-z-{z_short}");

    // Drop two PrRecord JSON files into prs-dir.
    write_record(
        &prs_dir,
        &feature_id,
        "alice",
        "feature",
        &feature_sha,
        &work,
    );
    write_record(
        &prs_dir,
        &feature_z_id,
        "alice",
        "feature-z",
        &z_sha,
        &work,
    );

    Fixture {
        _tmp: tmp,
        base_dir,
        central,
        prs_dir,
        feature_id,
        feature_z_id,
    }
}

fn write_record(
    prs_dir: &Path,
    id: &str,
    agent: &str,
    branch: &str,
    commit: &str,
    source_repo: &Path,
) {
    let body = json!({
        "id": id,
        "agent_id": agent,
        "source_repo": source_repo.to_string_lossy(),
        "branch": branch,
        "target": "main",
        "commit": commit,
        "summary": format!("{branch} ready"),
        "status": "open",
        "created_at": "2026-04-25T00:00:00Z",
    });
    let path = prs_dir.join(format!("{id}.json"));
    std::fs::write(path, serde_json::to_string_pretty(&body).unwrap()).unwrap();
}

struct ServerGuard {
    child: Option<Child>,
    port: u16,
}

impl ServerGuard {
    fn boot(fix: &Fixture) -> Self {
        let mut child = Command::new(server_binary())
            .arg("agents.toml")
            .arg("--prs-dir")
            .arg(&fix.prs_dir)
            .arg("--central-repo")
            .arg(&fix.central)
            .current_dir(&fix.base_dir)
            .env("ATN_PORT", "0")
            .env("RUST_LOG", "atn_server=warn")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn atn-server");

        let stdout = child.stdout.take().expect("server stdout");
        let reader = BufReader::new(stdout);
        let mut port: Option<u16> = None;
        for line in reader.lines().take(200).map_while(Result::ok) {
            if let Some(rest) = line.strip_prefix("atn-server ready on ")
                && let Some((_, p)) = rest.rsplit_once(':')
                && let Ok(parsed) = p.parse::<u16>()
            {
                port = Some(parsed);
                break;
            }
        }
        let port = port.expect("never saw `atn-server ready on ...`");

        Self {
            child: Some(child),
            port,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.port, path)
    }
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn curl(method: &str, url: &str, body: Option<&Value>) -> (u16, String) {
    let mut cmd = Command::new("curl");
    cmd.args(["-sS", "-o", "-", "-w", "\n__STATUS__=%{http_code}", "-X", method]);
    if let Some(b) = body {
        cmd.args(["-H", "Content-Type: application/json"]);
        cmd.args(["--data-binary", &b.to_string()]);
    }
    cmd.arg(url);
    let out = cmd.output().expect("curl");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let (body_part, status) = stdout
        .rsplit_once("__STATUS__=")
        .unwrap_or((stdout.as_str(), "0"));
    let body = body_part.trim_end_matches('\n').to_string();
    (status.trim().parse().unwrap_or(0), body)
}

fn poll_until<F: FnMut() -> bool>(timeout: Duration, mut f: F) -> bool {
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
fn prs_endpoints_round_trip() {
    let fix = build_fixture();
    let srv = ServerGuard::boot(&fix);

    // Wait for the server to actually accept.
    let healthy = poll_until(Duration::from_secs(5), || {
        let (code, _) = curl("GET", &srv.url("/api/prs"), None);
        code == 200
    });
    assert!(healthy, "server never returned 200 on /api/prs");

    // GET /api/prs (unfiltered): 2 entries, lexically sorted.
    let (code, body) = curl("GET", &srv.url("/api/prs"), None);
    assert_eq!(code, 200, "list body: {body}");
    let prs: Vec<Value> = serde_json::from_str(&body).unwrap();
    assert_eq!(prs.len(), 2, "expected 2 records, got {body}");
    let ids: Vec<&str> = prs.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&fix.feature_id.as_str()));
    assert!(ids.contains(&fix.feature_z_id.as_str()));
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted, "list should be lexically sorted");

    // GET /api/prs?status=open: still both.
    let (code, body) = curl("GET", &srv.url("/api/prs?status=open"), None);
    assert_eq!(code, 200);
    let prs: Vec<Value> = serde_json::from_str(&body).unwrap();
    assert_eq!(prs.len(), 2);

    // GET /api/prs?status=merged: zero.
    let (code, body) = curl("GET", &srv.url("/api/prs?status=merged"), None);
    assert_eq!(code, 200);
    let prs: Vec<Value> = serde_json::from_str(&body).unwrap();
    assert!(prs.is_empty(), "expected no merged PRs yet, got {body}");

    // GET /api/prs/{id}: 200 with the right record.
    let (code, body) =
        curl("GET", &srv.url(&format!("/api/prs/{}", fix.feature_id)), None);
    assert_eq!(code, 200, "get body: {body}");
    let pr: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(pr["branch"], "feature");
    assert_eq!(pr["status"], "open");

    // GET /api/prs/missing: 404.
    let (code, _body) = curl("GET", &srv.url("/api/prs/does-not-exist"), None);
    assert_eq!(code, 404);

    // POST /api/prs/{id}/merge: success path.
    let (code, body) = curl(
        "POST",
        &srv.url(&format!("/api/prs/{}/merge", fix.feature_id)),
        Some(&json!({})),
    );
    assert_eq!(code, 200, "merge body: {body}");
    let pr: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(pr["status"], "merged");
    assert!(pr["merge_commit"].as_str().unwrap().len() >= 7);
    assert!(pr["merged_at"].as_str().is_some());

    // Central main now has the new commit (feature.txt + merge).
    let log = run_git_capture(&fix.central, &["log", "--oneline", "main"]);
    assert!(
        log.contains("Merge refs/heads/pr/alice-feature"),
        "central main missing merge commit; log:\n{log}"
    );
    assert!(
        fix.central.join("feature.txt").exists(),
        "central worktree missing the merged feature.txt file"
    );

    // POST /api/prs/{id}/merge again: 409 (status is merged).
    let (code, body) = curl(
        "POST",
        &srv.url(&format!("/api/prs/{}/merge", fix.feature_id)),
        Some(&json!({})),
    );
    assert_eq!(code, 409, "second-merge body: {body}");

    // POST /api/prs/{id}/reject: status flips on the OTHER PR.
    let (code, body) = curl(
        "POST",
        &srv.url(&format!("/api/prs/{}/reject", fix.feature_z_id)),
        Some(&json!({})),
    );
    assert_eq!(code, 200, "reject body: {body}");
    let pr: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(pr["status"], "rejected");
    assert!(pr["rejected_at"].as_str().is_some());

    // ?status=rejected now returns the rejected one.
    let (code, body) = curl("GET", &srv.url("/api/prs?status=rejected"), None);
    assert_eq!(code, 200);
    let prs: Vec<Value> = serde_json::from_str(&body).unwrap();
    assert_eq!(prs.len(), 1);
    assert_eq!(prs[0]["id"], fix.feature_z_id);

    // POST /api/prs/missing/merge: 404.
    let (code, _) = curl(
        "POST",
        &srv.url("/api/prs/no-such-id/merge"),
        Some(&json!({})),
    );
    assert_eq!(code, 404);
}
