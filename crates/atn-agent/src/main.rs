//! atn-agent — Rust-native AI coding agent wrapper.
//!
//! Runs as an ATN `launch_command` on the agent side of a PTY.
//! Polls `<atn-dir>/inboxes/<agent-id>/` for new messages and (in
//! later saga steps) calls an Ollama-compatible LLM to produce
//! responses + tool calls. This step (cli-scaffold) covers the
//! lifecycle, CLI, banner, inbox polling, and graceful shutdown —
//! no HTTP calls to the LLM yet.
//!
//! # Exit codes
//!
//! - `0` clean exit (SIGINT / `--exit-on-empty` reached empty pass).
//! - `1` bad CLI args / workspace resolution error.
//! - `2` inbox or outbox directory couldn't be created.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use atn_core::inbox::{INBOXES_DIR, InboxMessage};
#[cfg(test)]
use atn_core::inbox::ATN_DIR_NAME;
use clap::Parser;

const EXIT_OK: u8 = 0;
const EXIT_USAGE: u8 = 1;
const EXIT_IO: u8 = 2;

#[derive(Parser, Debug)]
#[command(
    name = "atn-agent",
    version,
    about = "Rust-native AI coding agent wrapper for ATN",
    long_about = "Rust-native AI coding agent wrapper for ATN.\n\n\
                  Polls the per-agent inbox directory and drives a\n\
                  tool-calling LLM loop (Ollama /api/chat shape) to\n\
                  produce responses. Meant to be run as an ATN\n\
                  launch_command so the agent appears in the dashboard\n\
                  like any other PTY.\n\n\
                  Exit codes: 0 clean, 1 usage, 2 io."
)]
struct Cli {
    /// Agent id (matches the ATN agent id). Required.
    #[arg(long, value_name = "ID")]
    agent_id: String,

    /// Base URL for the Ollama-compatible /api/chat endpoint.
    /// Unused in saga step 1; lands in step 2.
    #[arg(long, default_value = "http://localhost:11434")]
    base_url: String,

    /// Model name to pass to the LLM (Ollama's `model` field).
    #[arg(long, default_value = "qwen3:8b")]
    model: String,

    /// Path to the ATN root (the directory containing `inboxes/` and
    /// `outboxes/`). Usually `.atn` inside the agent's working dir.
    #[arg(long, value_name = "PATH", default_value = ".atn")]
    atn_dir: PathBuf,

    /// Agent's workspace directory. Used by the `file_*` + shell tools
    /// (landing in saga steps 3–4) as a sandbox root.
    #[arg(long, value_name = "PATH", default_value = ".")]
    workspace: PathBuf,

    /// Seconds between inbox polls.
    #[arg(long, default_value_t = 2)]
    inbox_poll_secs: u64,

    /// Cap on per-message tool-call iterations (prevents infinite loops).
    /// Unused in step 1; lands in step 3.
    #[arg(long, default_value_t = 8)]
    max_tool_iterations: u32,

    /// Enable `shell_exec` tool. Off by default — the tool returns
    /// a "disabled" message to the model when unset.
    #[arg(long)]
    allow_shell: bool,

    /// Skip the LLM call. Each inbox hit prints a "would POST /api/chat"
    /// line and moves on. Still writes the .json.done rename.
    #[arg(long)]
    dry_run: bool,

    /// Exit cleanly after the first inbox pass that hands no messages.
    /// Intended for integration tests — run one-shot without needing a
    /// signal to tear the process down.
    #[arg(long)]
    exit_on_empty: bool,

    /// Log every inbox poll tick (normally silent between hits).
    #[arg(long)]
    verbose: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err((code, msg)) => {
            eprintln!("atn-agent: {msg}");
            ExitCode::from(code)
        }
    }
}

fn run(cli: Cli) -> Result<u8, (u8, String)> {
    // Resolve the inbox path once so errors are fast + clear.
    let inbox_dir = resolve_inbox_dir(&cli.atn_dir, &cli.agent_id)
        .map_err(|e| (EXIT_USAGE, e))?;
    std::fs::create_dir_all(&inbox_dir)
        .map_err(|e| (EXIT_IO, format!("create inbox dir {inbox_dir:?}: {e}")))?;

    // Banner tells ATN's PTY state tracker the agent is alive.
    println!(
        "atn-agent: {id} up (model={m}, base_url={b})",
        id = cli.agent_id,
        m = cli.model,
        b = cli.base_url
    );
    if cli.verbose {
        eprintln!(
            "atn-agent: inbox={inbox_dir} poll={poll}s dry_run={dr} allow_shell={as_}",
            inbox_dir = inbox_dir.display(),
            poll = cli.inbox_poll_secs,
            dr = cli.dry_run,
            as_ = cli.allow_shell,
        );
    }

    // Production tear-down is SIGINT from ATN (Ctrl-C via the PTY).
    // Rust's default handler exits the process cleanly, which is
    // exactly what ATN's restart/shutdown path expects. Tests use
    // `--exit-on-empty` to run one-shot without a signal.
    loop {
        let handled = poll_inbox_once(&inbox_dir, cli.verbose, cli.dry_run)
            .map_err(|e| (EXIT_IO, e))?;
        if cli.exit_on_empty && handled == 0 {
            if cli.verbose {
                eprintln!("atn-agent: --exit-on-empty + no messages, exiting");
            }
            return Ok(EXIT_OK);
        }
        std::thread::sleep(Duration::from_secs(cli.inbox_poll_secs));
    }
}

/// Resolve and validate the inbox directory path for an agent.
///
/// Returns the path `<atn_dir>/inboxes/<agent_id>` without touching
/// the filesystem. Rejects empty agent ids and ids containing path
/// separators or `..` (defense-in-depth — the server never hands
/// these out, but atn-agent runs with agent-supplied args).
pub(crate) fn resolve_inbox_dir(atn_dir: &Path, agent_id: &str) -> Result<PathBuf, String> {
    if agent_id.is_empty() {
        return Err("--agent-id cannot be empty".into());
    }
    if agent_id.contains('/') || agent_id.contains('\\') || agent_id.contains("..") {
        return Err(format!("--agent-id {agent_id:?} contains forbidden characters"));
    }
    Ok(atn_dir.join(INBOXES_DIR).join(agent_id))
}

/// Scan the inbox once. Returns the number of handled messages.
fn poll_inbox_once(inbox_dir: &Path, verbose: bool, dry_run: bool) -> Result<usize, String> {
    if verbose {
        eprintln!("atn-agent: polling {}", inbox_dir.display());
    }
    let entries = match std::fs::read_dir(inbox_dir) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(format!("read_dir {inbox_dir:?}: {e}")),
    };
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) == Some("json") {
            paths.push(p);
        }
    }
    // Deterministic order — the ATN message router writes
    // `<event-id>.json`; event ids include timestamps, so lexical sort
    // is timeline order.
    paths.sort();
    let handled = paths.len();
    for path in paths {
        handle_inbox_file(&path, dry_run)?;
    }
    Ok(handled)
}

fn handle_inbox_file(path: &Path, dry_run: bool) -> Result<(), String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read {path:?}: {e}"))?;
    let msg: InboxMessage = match serde_json::from_str(&raw) {
        Ok(m) => m,
        Err(e) => {
            // Log + skip so a single malformed file doesn't wedge the
            // poll loop. Leave the file in place so an operator can
            // repair it.
            eprintln!("atn-agent: skipping malformed {}: {e}", path.display());
            return Ok(());
        }
    };
    println!(
        "inbox: {id} — {summary}",
        id = msg.event.id,
        summary = msg.event.summary.replace('\n', " ").chars().take(160).collect::<String>(),
    );
    if dry_run {
        println!("atn-agent: would POST /api/chat for {}", msg.event.id);
    }
    // Rename to .json.done so the next poll doesn't reprocess.
    let done = path.with_extension("json.done");
    std::fs::rename(path, &done)
        .map_err(|e| format!("rename {path:?} -> {done:?}: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_inbox_dir_appends_path_segments() {
        let p = resolve_inbox_dir(Path::new(".atn"), "worker-hlasm").unwrap();
        assert_eq!(p, PathBuf::from(".atn/inboxes/worker-hlasm"));
    }

    #[test]
    fn resolve_inbox_dir_rejects_empty_agent_id() {
        assert!(resolve_inbox_dir(Path::new(".atn"), "").is_err());
    }

    #[test]
    fn resolve_inbox_dir_rejects_path_traversal() {
        for bad in ["../escape", "a/b", "a\\b", ".."] {
            assert!(
                resolve_inbox_dir(Path::new(".atn"), bad).is_err(),
                "expected rejection for {bad:?}"
            );
        }
    }

    #[test]
    fn poll_inbox_handles_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = resolve_inbox_dir(tmp.path(), "demo").unwrap();
        std::fs::create_dir_all(&inbox).unwrap();
        assert_eq!(poll_inbox_once(&inbox, false, false).unwrap(), 0);
    }

    #[test]
    fn poll_inbox_renames_json_to_done() {
        use atn_core::event::{Priority, PushEvent, PushKind};
        let tmp = tempfile::tempdir().unwrap();
        let inbox = resolve_inbox_dir(tmp.path(), "demo").unwrap();
        std::fs::create_dir_all(&inbox).unwrap();
        let msg = InboxMessage {
            event: PushEvent {
                id: "ev-1".into(),
                kind: PushKind::FeatureRequest,
                source_agent: "coord".into(),
                source_repo: ".".into(),
                target_agent: Some("demo".into()),
                issue_id: None,
                summary: "hello".into(),
                wiki_link: None,
                priority: Priority::Normal,
                timestamp: "2026-04-24T10:00:00Z".into(),
            },
            delivered: true,
            delivered_at: Some("2026-04-24T10:00:01Z".into()),
        };
        std::fs::write(
            inbox.join("ev-1.json"),
            serde_json::to_string_pretty(&msg).unwrap(),
        )
        .unwrap();
        assert_eq!(poll_inbox_once(&inbox, false, true).unwrap(), 1);
        assert!(inbox.join("ev-1.json.done").exists());
        assert!(!inbox.join("ev-1.json").exists());
    }

    #[test]
    fn poll_inbox_skips_malformed_json() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = resolve_inbox_dir(tmp.path(), "demo").unwrap();
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::write(inbox.join("bad.json"), "{not-json").unwrap();
        // Handler counts the file as "handled" (we iterated it) but
        // leaves it on disk for the operator to inspect.
        assert_eq!(poll_inbox_once(&inbox, false, false).unwrap(), 1);
        assert!(inbox.join("bad.json").exists());
    }

    // Sanity check that ATN_DIR_NAME is still what we expect —
    // clap default for --atn-dir hardcodes it, so a rename in
    // atn-core would quietly break the agent.
    #[test]
    fn atn_dir_const_matches_hardcoded_default() {
        assert_eq!(ATN_DIR_NAME, ".atn");
    }
}
