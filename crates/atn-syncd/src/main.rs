//! atn-syncd — out-of-band sync agent.
//!
//! Watches an agent's git worktree for a `.atn-ready-to-pr` marker
//! file. When the marker shows up, the daemon (in step 2) pushes
//! the named branch to a configured central remote and writes a
//! `PrRecord` to `<prs-dir>/<id>.json` so the dashboard + atn-cli
//! can take it from there.
//!
//! Step 1 (this commit) wires the lifecycle, CLI, and the watcher
//! loop. The handler is a stub that just logs `would handle …`;
//! step 2 fills in the git push + record write.
//!
//! # Exit codes
//!
//! - `0` clean exit (`--exit-on-empty` or SIGINT)
//! - `1` usage / path-resolution error
//! - `2` IO error setting up watch directories

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

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

    println!(
        "atn-syncd: {id} watching {repo} (marker={marker}, prs-dir={prs})",
        id = cli.agent_id,
        repo = repo.display(),
        marker = cli.marker,
        prs = cli.prs_dir.display(),
    );
    if cli.verbose {
        eprintln!(
            "atn-syncd: poll={poll}s remote={remote} dry_run={dr}",
            poll = cli.poll_secs,
            remote = cli.remote,
            dr = cli.dry_run,
        );
    }

    loop {
        let saw_marker = poll_once(&marker_path, cli.dry_run, cli.verbose);
        if cli.exit_on_empty && !saw_marker {
            if cli.verbose {
                eprintln!("atn-syncd: --exit-on-empty + no marker, exiting");
            }
            return Ok(EXIT_OK);
        }
        std::thread::sleep(Duration::from_secs(cli.poll_secs));
    }
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
/// (handler stub will action it; step 2 wires the real push).
fn poll_once(marker_path: &Path, dry_run: bool, verbose: bool) -> bool {
    if verbose {
        eprintln!("atn-syncd: checking {}", marker_path.display());
    }
    if marker_path.exists() {
        if dry_run {
            println!(
                "atn-syncd: would handle marker at {} (dry-run)",
                marker_path.display()
            );
        } else {
            println!(
                "atn-syncd: marker present at {} (handler stub — step 2 wires push+record)",
                marker_path.display()
            );
        }
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn poll_once_returns_true_when_marker_present() {
        let tmp = tempfile::tempdir().unwrap();
        let marker = tmp.path().join(".atn-ready-to-pr");
        assert!(!poll_once(&marker, false, false));
        std::fs::write(&marker, "branch=feature\n").unwrap();
        assert!(poll_once(&marker, false, false));
    }

    #[test]
    fn pr_record_module_is_reachable() {
        // Confirms atn-core::pr is wired through; step 2 uses it.
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
}
