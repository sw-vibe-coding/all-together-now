//! End-to-end test for `atn-syncd` (git-sync-agents step 5).
//!
//! Spawns the real `atn-syncd` binary against a tempdir layout with
//! a bare central remote, an agent worktree, and a `prs-dir`, drops
//! a marker, then asserts the bare remote received the branch and a
//! `PrRecord` JSON appeared on disk. Exercises the same flow `Demo
//! 13` walks through, but in a couple hundred ms instead of a sleep
//! loop.

use std::path::{Path, PathBuf};
use std::process::Command;

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

fn build_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let central = tmp.path().join("central.git");
    let work = tmp.path().join("alice");
    let prs_dir = tmp.path().join("prs");
    std::fs::create_dir_all(&prs_dir).unwrap();

    // Bare central remote.
    Command::new("git")
        .args(["init", "--bare"])
        .arg(&central)
        .output()
        .unwrap();

    // Agent worktree with one commit on a feature branch.
    Command::new("git")
        .args(["init", "--initial-branch=main"])
        .arg(&work)
        .output()
        .unwrap();
    run_git(&work, &["config", "user.email", "alice@test"]);
    run_git(&work, &["config", "user.name", "Alice"]);
    run_git(&work, &["config", "commit.gpgsign", "false"]);
    std::fs::write(work.join("README.md"), "alice repo\n").unwrap();
    run_git(&work, &["add", "README.md"]);
    run_git(&work, &["commit", "-m", "init"]);
    run_git(&work, &["checkout", "-b", "feature"]);
    std::fs::write(work.join("feature.txt"), "feature\n").unwrap();
    run_git(&work, &["add", "feature.txt"]);
    run_git(&work, &["commit", "-m", "add feature"]);
    run_git(
        &work,
        &[
            "remote",
            "add",
            "central",
            central.to_str().unwrap(),
        ],
    );

    (tmp, central, work, prs_dir)
}

fn run_syncd_once(work: &Path, prs_dir: &Path) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_atn-syncd");
    Command::new(bin)
        .arg("--repo")
        .arg(work)
        .arg("--agent-id")
        .arg("alice")
        .arg("--remote")
        .arg("central")
        .arg("--prs-dir")
        .arg(prs_dir)
        .arg("--poll-secs")
        .arg("1")
        .arg("--exit-on-empty")
        .arg("--verbose")
        .output()
        .expect("spawn atn-syncd")
}

#[test]
fn syncd_end_to_end_pushes_and_records() {
    let (_tmp, central, work, prs_dir) = build_fixture();

    // Drop the marker BEFORE running syncd. With --exit-on-empty,
    // syncd polls once, sees the marker, handles it, polls again,
    // sees it absent (renamed), and exits 0.
    let marker = work.join(".atn-ready-to-pr");
    std::fs::write(&marker, "summary=demo PR from alice\n").unwrap();

    let out = run_syncd_once(&work, &prs_dir);
    assert!(
        out.status.success(),
        "syncd exit {:?}; stdout={}, stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // Banner + handler line should be on stdout.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("atn-syncd: alice watching"),
        "missing banner: {stdout}"
    );
    assert!(
        stdout.contains("pushed feature → pr/alice-feature-"),
        "missing handler line: {stdout}"
    );

    // Bare remote has the pushed ref.
    let refs = run_git_capture(&central, &["for-each-ref", "--format=%(refname)"]);
    assert!(
        refs.contains("refs/heads/pr/alice-feature"),
        "central remote missing pushed branch; got refs:\n{refs}"
    );

    // PR record JSON exists, round-trips, and points at the right SHA.
    let pr_files: Vec<PathBuf> = std::fs::read_dir(&prs_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    assert_eq!(pr_files.len(), 1, "expected 1 PR record, got {pr_files:?}");
    let body = std::fs::read_to_string(&pr_files[0]).unwrap();
    let pr: atn_core::pr::PrRecord = serde_json::from_str(&body).unwrap();
    assert_eq!(pr.agent_id, "alice");
    assert_eq!(pr.branch, "feature");
    assert_eq!(pr.target, "main");
    assert_eq!(pr.summary, "demo PR from alice");
    assert_eq!(pr.status, atn_core::pr::PrStatus::Open);
    let want_sha = run_git_capture(&work, &["rev-parse", "feature"]);
    assert_eq!(pr.commit, want_sha);
    let short: String = want_sha.chars().take(7).collect();
    assert_eq!(pr.id, format!("alice-feature-{short}"));

    // Marker has been renamed to .queued.<short>.
    assert!(!marker.exists(), "marker should be moved out of place");
    let queued = work.join(format!(".atn-ready-to-pr.queued.{short}"));
    assert!(queued.exists(), "expected {} to exist", queued.display());
}

#[test]
fn syncd_no_marker_exits_clean() {
    let (_tmp, _central, work, prs_dir) = build_fixture();
    let out = run_syncd_once(&work, &prs_dir);
    assert!(
        out.status.success(),
        "syncd should exit 0 when no marker is present; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let entries: Vec<_> = std::fs::read_dir(&prs_dir).unwrap().collect();
    assert!(
        entries.iter().all(|e| e.is_ok()),
        "prs-dir read failed: {entries:?}"
    );
    assert_eq!(
        entries.len(),
        0,
        "no record should land when no marker present"
    );
}
