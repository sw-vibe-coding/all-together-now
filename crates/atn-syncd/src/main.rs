//! atn-syncd — out-of-band sync agent.
//!
//! Watches an agent's git worktree for a `.atn-ready-to-pr` marker
//! file. When the marker shows up, the daemon pushes the named
//! branch to a configured central remote and writes a `PrRecord`
//! to `<prs-dir>/<id>.json` so the dashboard + atn-cli can take it
//! from there.
//!
//! # Lifecycle
//!
//! 1. Marker present → parse `key=value` body (`branch`, `target`,
//!    `summary`; defaults fill in for missing keys).
//! 2. `git push <remote> <branch>:refs/heads/pr/<agent-id>-<branch>`.
//!    Push errors leave the marker in place — next poll retries.
//! 3. `git rev-parse <branch>` → SHA → `id = <agent-id>-<branch>-<short>`.
//! 4. Write `<prs-dir>/<id>.json` (PrRecord, status=Open).
//! 5. Rename `<repo>/<marker>` → `<repo>/<marker>.queued.<short>` so
//!    the same marker isn't re-processed.
//!
//! # Exit codes
//!
//! - `0` clean exit (`--exit-on-empty` or SIGINT)
//! - `1` usage / path-resolution error
//! - `2` IO error setting up watch directories

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use atn_core::pr::{PrRecord, PrStatus};
use clap::Parser;

const EXIT_OK: u8 = 0;
const EXIT_USAGE: u8 = 1;
const EXIT_IO: u8 = 2;

#[derive(Parser, Debug)]
#[command(
    name = "atn-syncd",
    version,
    about = "ATN out-of-band sync daemon",
    long_about = "Watches an agent's git worktree for a marker file. When the\n\
                  marker shows up, the daemon pushes the branch to a central\n\
                  remote and writes a PR record into <prs-dir>/<id>.json so\n\
                  the dashboard and atn-cli can act on it.\n\n\
                  Exit codes: 0 clean, 1 usage, 2 io."
)]
struct Cli {
    /// Path to the agent's git worktree.
    #[arg(long, value_name = "PATH")]
    repo: PathBuf,

    /// Agent id; used to namespace pushed branches as `pr/<id>-<branch>`.
    #[arg(long, value_name = "ID")]
    agent_id: String,

    /// Git remote name on the agent's repo (`git push <remote> …`).
    #[arg(long, default_value = "central")]
    remote: String,

    /// Marker filename, relative to the repo root.
    #[arg(long, default_value = ".atn-ready-to-pr")]
    marker: String,

    /// Where to write `PrRecord` JSON files. Used by atn-server's
    /// `/api/prs` endpoint in step 3.
    #[arg(long, value_name = "PATH", default_value = ".atn/prs")]
    prs_dir: PathBuf,

    /// Seconds between watch ticks.
    #[arg(long, default_value_t = 3)]
    poll_secs: u64,

    /// Skip the push + write; just log "would handle …".
    #[arg(long)]
    dry_run: bool,

    /// Exit cleanly after the first marker-free pass. Used by tests.
    #[arg(long)]
    exit_on_empty: bool,

    /// Log every poll tick (otherwise quiet between marker hits).
    #[arg(long)]
    verbose: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err((code, msg)) => {
            eprintln!("atn-syncd: {msg}");
            ExitCode::from(code)
        }
    }
}

fn run(cli: Cli) -> Result<u8, (u8, String)> {
    let repo = cli
        .repo
        .canonicalize()
        .map_err(|e| (EXIT_USAGE, format!("invalid --repo {:?}: {e}", cli.repo)))?;
    if cli.agent_id.trim().is_empty()
        || cli.agent_id.contains('/')
        || cli.agent_id.contains('\\')
        || cli.agent_id.contains("..")
    {
        return Err((
            EXIT_USAGE,
            format!("--agent-id {:?} contains forbidden characters", cli.agent_id),
        ));
    }
    let marker_path = resolve_marker_path(&repo, &cli.marker)?;
    std::fs::create_dir_all(&cli.prs_dir).map_err(|e| {
        (
            EXIT_IO,
            format!("create --prs-dir {:?}: {e}", cli.prs_dir),
        )
    })?;
    let prs_dir = cli.prs_dir.canonicalize().map_err(|e| {
        (
            EXIT_IO,
            format!("canonicalize --prs-dir {:?}: {e}", cli.prs_dir),
        )
    })?;

    println!(
        "atn-syncd: {id} watching {repo} (marker={marker}, prs-dir={prs})",
        id = cli.agent_id,
        repo = repo.display(),
        marker = cli.marker,
        prs = prs_dir.display(),
    );
    if cli.verbose {
        eprintln!(
            "atn-syncd: poll={poll}s remote={remote} dry_run={dr}",
            poll = cli.poll_secs,
            remote = cli.remote,
            dr = cli.dry_run,
        );
    }

    let ctx = HandlerCtx {
        repo,
        agent_id: cli.agent_id,
        remote: cli.remote,
        prs_dir,
        dry_run: cli.dry_run,
        verbose: cli.verbose,
    };

    loop {
        let saw_marker = poll_once(&marker_path, &ctx);
        if cli.exit_on_empty && !saw_marker {
            if cli.verbose {
                eprintln!("atn-syncd: --exit-on-empty + no marker, exiting");
            }
            return Ok(EXIT_OK);
        }
        std::thread::sleep(Duration::from_secs(cli.poll_secs));
    }
}

/// Per-loop runtime context shared with the marker handler.
pub(crate) struct HandlerCtx {
    pub repo: PathBuf,
    pub agent_id: String,
    pub remote: String,
    pub prs_dir: PathBuf,
    pub dry_run: bool,
    pub verbose: bool,
}

/// Resolve the marker path against the repo root, refusing escape
/// attempts. The marker is meant to live INSIDE the worktree —
/// anything resolving outside is a config bug.
pub(crate) fn resolve_marker_path(
    repo: &Path,
    marker: &str,
) -> Result<PathBuf, (u8, String)> {
    if marker.trim().is_empty() {
        return Err((EXIT_USAGE, "--marker cannot be empty".into()));
    }
    if Path::new(marker).is_absolute() {
        return Err((
            EXIT_USAGE,
            format!("--marker must be repo-relative, got {marker:?}"),
        ));
    }
    if marker.contains("..") {
        return Err((
            EXIT_USAGE,
            format!("--marker {marker:?} cannot traverse parents"),
        ));
    }
    Ok(repo.join(marker))
}

/// One watch tick. Returns `true` if the marker file was present
/// (handler ran or stub-logged in dry-run).
fn poll_once(marker_path: &Path, ctx: &HandlerCtx) -> bool {
    if ctx.verbose {
        eprintln!("atn-syncd: checking {}", marker_path.display());
    }
    if !marker_path.exists() {
        return false;
    }
    if ctx.dry_run {
        println!(
            "atn-syncd: would handle marker at {} (dry-run)",
            marker_path.display()
        );
        return true;
    }
    match handle_marker(marker_path, ctx) {
        Ok(outcome) => {
            println!(
                "atn-syncd: pushed {branch} → pr/{id} (commit {sha}); record {file}; queued {q}",
                branch = outcome.branch,
                id = outcome.pr_id,
                sha = outcome.short_sha,
                file = outcome.record_path.display(),
                q = outcome.renamed_marker.display(),
            );
        }
        Err(e) => {
            eprintln!("atn-syncd: handle_marker failed: {e}");
        }
    }
    true
}

/// What `handle_marker` produced on success — used by the loop to
/// log a one-line summary and by tests to assert state.
#[derive(Debug, Clone)]
pub(crate) struct HandleOutcome {
    pub branch: String,
    pub pr_id: String,
    pub short_sha: String,
    pub record_path: PathBuf,
    pub renamed_marker: PathBuf,
}

/// Body of a marker file: empty marker is fine, defaults fill in.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct MarkerSpec {
    pub branch: Option<String>,
    pub target: Option<String>,
    pub summary: Option<String>,
}

/// Parse a marker file's body. Format: one `key=value` per line.
/// `#`-prefixed lines and blank lines are ignored. Unknown keys
/// are tolerated (so older agents can leave stuff for newer
/// daemons without blowing up). Whitespace around `=` is trimmed.
pub(crate) fn parse_marker(content: &str) -> MarkerSpec {
    let mut spec = MarkerSpec::default();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let val = v.trim().to_string();
        if val.is_empty() {
            continue;
        }
        match key {
            "branch" => spec.branch = Some(val),
            "target" => spec.target = Some(val),
            "summary" => spec.summary = Some(val),
            _ => {}
        }
    }
    spec
}

/// Run `git` in `repo` with `args`. On failure, returns a string
/// containing the captured stderr (and a hint at the exit code).
fn run_git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("git {:?} spawn: {e}", args))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!(
            "git {} → exit {:?}: {stderr}",
            args.join(" "),
            out.status.code()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Resolve the worktree's current branch (`HEAD`-symref).
fn current_branch(repo: &Path) -> Result<String, String> {
    run_git(repo, &["rev-parse", "--abbrev-ref", "HEAD"])
}

/// Marker → push → record → rename. All-or-nothing semantics:
/// any failure leaves the marker in place so the next poll retries.
pub(crate) fn handle_marker(
    marker_path: &Path,
    ctx: &HandlerCtx,
) -> Result<HandleOutcome, String> {
    let body = std::fs::read_to_string(marker_path)
        .map_err(|e| format!("read marker {:?}: {e}", marker_path))?;
    let spec = parse_marker(&body);
    let branch = match spec.branch {
        Some(b) => b,
        None => current_branch(&ctx.repo)?,
    };
    if branch.is_empty() || branch == "HEAD" {
        return Err(format!("refusing to push detached/empty branch ({branch:?})"));
    }
    let target = spec.target.unwrap_or_else(|| "main".to_string());
    let summary = spec
        .summary
        .unwrap_or_else(|| format!("{branch} ready for review"));

    let pr_ref = format!("pr/{}-{}", ctx.agent_id, branch);
    let refspec = format!("{branch}:refs/heads/{pr_ref}");
    if ctx.verbose {
        eprintln!(
            "atn-syncd: git push {} {} (in {})",
            ctx.remote,
            refspec,
            ctx.repo.display()
        );
    }
    run_git(&ctx.repo, &["push", &ctx.remote, &refspec])?;

    let full_sha = run_git(&ctx.repo, &["rev-parse", &branch])?;
    let short_sha = short_sha(&full_sha);
    let pr_id = format!("{}-{}-{}", ctx.agent_id, branch, short_sha);

    let created_at = chrono::Utc::now().to_rfc3339();
    let record = PrRecord {
        id: pr_id.clone(),
        agent_id: ctx.agent_id.clone(),
        source_repo: ctx.repo.display().to_string(),
        branch: branch.clone(),
        target,
        commit: full_sha,
        summary,
        status: PrStatus::Open,
        created_at,
        merge_commit: None,
        merged_at: None,
        rejected_at: None,
        last_error: None,
    };
    let record_path = ctx.prs_dir.join(record.filename());
    let json = serde_json::to_string_pretty(&record)
        .map_err(|e| format!("serialize PrRecord: {e}"))?;
    std::fs::write(&record_path, json)
        .map_err(|e| format!("write {:?}: {e}", record_path))?;

    let queued_path = marker_path.with_extension(format!("queued.{short_sha}"));
    if queued_path.exists() {
        eprintln!(
            "atn-syncd: queued marker {} already exists; leaving original in place",
            queued_path.display()
        );
        return Ok(HandleOutcome {
            branch,
            pr_id,
            short_sha,
            record_path,
            renamed_marker: queued_path,
        });
    }
    std::fs::rename(marker_path, &queued_path).map_err(|e| {
        format!("rename {:?} → {:?}: {e}", marker_path, queued_path)
    })?;

    Ok(HandleOutcome {
        branch,
        pr_id,
        short_sha,
        record_path,
        renamed_marker: queued_path,
    })
}

/// First 7 chars of a sha (or the whole thing if it's shorter).
fn short_sha(full: &str) -> String {
    full.chars().take(7).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn resolve_marker_path_joins_relative() {
        let tmp = tempfile::tempdir().unwrap();
        let p = resolve_marker_path(tmp.path(), ".atn-ready-to-pr").unwrap();
        assert_eq!(p, tmp.path().join(".atn-ready-to-pr"));
    }

    #[test]
    fn resolve_marker_path_rejects_empty() {
        assert!(resolve_marker_path(Path::new("/tmp"), "").is_err());
    }

    #[test]
    fn resolve_marker_path_rejects_absolute() {
        assert!(
            resolve_marker_path(Path::new("/tmp"), "/etc/passwd").is_err()
        );
    }

    #[test]
    fn resolve_marker_path_rejects_parent_traversal() {
        for bad in ["..", "../escape", "a/../escape"] {
            assert!(
                resolve_marker_path(Path::new("/tmp"), bad).is_err(),
                "expected reject for {bad:?}"
            );
        }
    }

    #[test]
    fn parse_marker_empty_yields_defaults() {
        let s = parse_marker("");
        assert_eq!(s, MarkerSpec::default());
    }

    #[test]
    fn parse_marker_reads_known_keys() {
        let s = parse_marker(
            "branch=feature-x\n\
             target=develop\n\
             summary=feature x ready\n",
        );
        assert_eq!(s.branch.as_deref(), Some("feature-x"));
        assert_eq!(s.target.as_deref(), Some("develop"));
        assert_eq!(s.summary.as_deref(), Some("feature x ready"));
    }

    #[test]
    fn parse_marker_tolerates_blanks_comments_unknown() {
        let s = parse_marker(
            "# leading comment\n\
             \n\
             branch = feature-y\n\
             nope=ignored\n\
             # another\n\
             summary= multi word summary  \n",
        );
        assert_eq!(s.branch.as_deref(), Some("feature-y"));
        assert!(s.target.is_none());
        assert_eq!(s.summary.as_deref(), Some("multi word summary"));
    }

    #[test]
    fn parse_marker_skips_empty_values() {
        let s = parse_marker("branch=\ntarget=main\n");
        assert!(s.branch.is_none());
        assert_eq!(s.target.as_deref(), Some("main"));
    }

    #[test]
    fn short_sha_truncates_to_seven() {
        assert_eq!(short_sha("abcdef0123456789"), "abcdef0");
        assert_eq!(short_sha("abc"), "abc");
    }

    #[test]
    fn pr_record_module_is_reachable() {
        // Confirms atn-core::pr is wired through; smoke check.
        use atn_core::pr::{PrRecord, PrStatus};
        let pr = PrRecord {
            id: "x".into(),
            agent_id: "alice".into(),
            source_repo: ".".into(),
            branch: "f".into(),
            target: "main".into(),
            commit: "abc".into(),
            summary: "s".into(),
            status: PrStatus::Open,
            created_at: "t".into(),
            merge_commit: None,
            merged_at: None,
            rejected_at: None,
            last_error: None,
        };
        let s = serde_json::to_string(&pr).unwrap();
        assert!(s.contains("\"open\""));
    }

    /// Spin up a worktree + bare central remote with one commit on
    /// `main`. Returns (tmp_root, worktree_path, central_path).
    fn fixture_repo() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let central = tmp.path().join("central.git");
        let worktree = tmp.path().join("worktree");
        run_git(tmp.path(), &["init", "--bare", central.to_str().unwrap()]).unwrap();
        run_git(
            tmp.path(),
            &[
                "init",
                "--initial-branch=main",
                worktree.to_str().unwrap(),
            ],
        )
        .unwrap();
        // Quiet local config so commit() doesn't depend on global state.
        run_git(&worktree, &["config", "user.email", "test@example.com"]).unwrap();
        run_git(&worktree, &["config", "user.name", "Test User"]).unwrap();
        run_git(&worktree, &["config", "commit.gpgsign", "false"]).unwrap();
        fs::write(worktree.join("README.md"), "hello\n").unwrap();
        run_git(&worktree, &["add", "README.md"]).unwrap();
        run_git(&worktree, &["commit", "-m", "init"]).unwrap();
        run_git(
            &worktree,
            &[
                "remote",
                "add",
                "central",
                central.to_str().unwrap(),
            ],
        )
        .unwrap();
        (tmp, worktree, central)
    }

    fn make_ctx(worktree: &Path, prs_dir: PathBuf) -> HandlerCtx {
        HandlerCtx {
            repo: worktree.to_path_buf(),
            agent_id: "alice".to_string(),
            remote: "central".to_string(),
            prs_dir,
            dry_run: false,
            verbose: false,
        }
    }

    #[test]
    fn handle_marker_pushes_and_records() {
        let (tmp, worktree, central) = fixture_repo();
        let prs_dir = tmp.path().join("prs");
        fs::create_dir_all(&prs_dir).unwrap();
        let marker = worktree.join(".atn-ready-to-pr");
        fs::write(&marker, "summary=hello world\n").unwrap();

        let ctx = make_ctx(&worktree, prs_dir.clone());
        let outcome = handle_marker(&marker, &ctx).expect("handler succeeded");

        assert_eq!(outcome.branch, "main");
        assert_eq!(outcome.pr_id, format!("alice-main-{}", outcome.short_sha));

        // (a) Central remote has refs/heads/pr/alice-main.
        let refs = run_git(&central, &["for-each-ref", "--format=%(refname)"]).unwrap();
        assert!(
            refs.contains("refs/heads/pr/alice-main"),
            "expected pushed ref, got {refs:?}"
        );

        // (b) The on-disk record parses back into the expected shape.
        let raw = fs::read_to_string(&outcome.record_path).unwrap();
        let pr: PrRecord = serde_json::from_str(&raw).unwrap();
        assert_eq!(pr.id, outcome.pr_id);
        assert_eq!(pr.branch, "main");
        assert_eq!(pr.target, "main");
        assert_eq!(pr.summary, "hello world");
        assert_eq!(pr.status, PrStatus::Open);
        assert!(pr.commit.starts_with(&outcome.short_sha));
        assert!(pr.merge_commit.is_none());

        // (c) Marker renamed to .queued.<short>; original gone.
        assert!(!marker.exists(), "original marker should be gone");
        let expected_queued =
            marker.with_extension(format!("queued.{}", outcome.short_sha));
        assert!(
            expected_queued.exists(),
            "expected {expected_queued:?} to exist"
        );
        assert_eq!(outcome.renamed_marker, expected_queued);
    }

    #[test]
    fn handle_marker_uses_marker_overrides() {
        let (tmp, worktree, _central) = fixture_repo();
        // Make a feature branch; HEAD is still main.
        run_git(&worktree, &["checkout", "-b", "feature-z"]).unwrap();
        fs::write(worktree.join("note.txt"), "z\n").unwrap();
        run_git(&worktree, &["add", "note.txt"]).unwrap();
        run_git(&worktree, &["commit", "-m", "z"]).unwrap();
        run_git(&worktree, &["checkout", "main"]).unwrap();

        let prs_dir = tmp.path().join("prs");
        fs::create_dir_all(&prs_dir).unwrap();
        let marker = worktree.join(".atn-ready-to-pr");
        fs::write(
            &marker,
            "branch=feature-z\ntarget=develop\nsummary=z PR\n",
        )
        .unwrap();

        let ctx = make_ctx(&worktree, prs_dir.clone());
        let outcome = handle_marker(&marker, &ctx).unwrap();
        assert_eq!(outcome.branch, "feature-z");

        let raw = fs::read_to_string(&outcome.record_path).unwrap();
        let pr: PrRecord = serde_json::from_str(&raw).unwrap();
        assert_eq!(pr.branch, "feature-z");
        assert_eq!(pr.target, "develop");
        assert_eq!(pr.summary, "z PR");
        assert_eq!(pr.id, format!("alice-feature-z-{}", outcome.short_sha));
    }

    #[test]
    fn handle_marker_push_failure_keeps_marker() {
        let (tmp, worktree, _central) = fixture_repo();
        // Point `central` remote at a path that doesn't exist.
        run_git(&worktree, &["remote", "remove", "central"]).unwrap();
        run_git(
            &worktree,
            &[
                "remote",
                "add",
                "central",
                tmp.path()
                    .join("does-not-exist.git")
                    .to_str()
                    .unwrap(),
            ],
        )
        .unwrap();
        let prs_dir = tmp.path().join("prs");
        fs::create_dir_all(&prs_dir).unwrap();
        let marker = worktree.join(".atn-ready-to-pr");
        fs::write(&marker, "").unwrap();

        let ctx = make_ctx(&worktree, prs_dir.clone());
        let err = handle_marker(&marker, &ctx).unwrap_err();
        assert!(err.contains("git push"), "unexpected err: {err}");
        // Marker stays so the next poll retries.
        assert!(marker.exists(), "marker should be preserved on push failure");
        // No record on disk.
        let entries: Vec<_> = fs::read_dir(&prs_dir)
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(entries.is_empty(), "no record should land on push failure");
    }
}
