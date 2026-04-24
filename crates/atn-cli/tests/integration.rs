//! End-to-end integration tests for atn-cli (saga step 5).
//!
//! Spawns a real `atn-server` on an ephemeral port, POSTs a bash
//! agent, then drives every atn-cli subcommand group with
//! `Command::new(env!("CARGO_BIN_EXE_atn-cli"))` and asserts on
//! captured stdout + exit codes. No Playwright or external state.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Resolve the repo root from `CARGO_MANIFEST_DIR` (crates/atn-cli).
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

/// RAII guard that kills the server subprocess when the test ends.
struct ServerGuard {
    child: Option<Child>,
    port: u16,
    _tmp: tempfile::TempDir,
}

impl ServerGuard {
    fn boot() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base_dir = tmp.path().to_path_buf();
        std::fs::write(
            base_dir.join("agents.toml"),
            "[project]\nname = \"atn-cli-test\"\nlog_dir = \".atn/logs\"\n",
        )
        .expect("write agents.toml");

        let tools = repo_root().join("tools");
        let path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{path}", tools.display());

        let mut child = Command::new(server_binary())
            .arg("agents.toml")
            .current_dir(&base_dir)
            .env("ATN_PORT", "0")
            .env("PATH", new_path)
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
            _tmp: tmp,
        }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
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

/// Run atn-cli with `ATN_URL` pointed at the test server. Returns
/// (exit_code, stdout, stderr).
fn run_cli(base_url: &str, args: &[&str], stdin: Option<&str>) -> (i32, String, String) {
    let bin = env!("CARGO_BIN_EXE_atn-cli");
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .env("ATN_URL", base_url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    let mut child = cmd.spawn().expect("spawn atn-cli");
    if let Some(data) = stdin
        && let Some(mut stdin_handle) = child.stdin.take()
    {
        stdin_handle.write_all(data.as_bytes()).expect("write stdin");
    }
    let output = child.wait_with_output().expect("wait atn-cli");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// Minimal curl helper for seeding the agent via REST. We could use
/// atn-cli itself to create agents, but there's no `agents create`
/// subcommand in this saga's scope — keep the harness decoupled.
fn curl_post(url: &str, body: &str) -> (u16, String) {
    let out = Command::new("curl")
        .args([
            "-sS",
            "-o",
            "-",
            "-w",
            "\n__STATUS__=%{http_code}",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            body,
            url,
        ])
        .output()
        .expect("curl");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let (body, status) = match stdout.rsplit_once("__STATUS__=") {
        Some((b, s)) => (b.trim_end_matches('\n').to_string(), s.trim()),
        None => (stdout, "0"),
    };
    (status.parse().unwrap_or(0), body)
}

fn poll_until<F: FnMut() -> bool>(timeout: Duration, mut f: F) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    f()
}

#[test]
fn atn_cli_end_to_end_tour() {
    let srv = ServerGuard::boot();
    let url = srv.base_url();

    // 1. Empty agents list.
    let (code, stdout, _) = run_cli(&url, &["agents", "list"], None);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("(no agents)"),
        "empty list: {stdout:?}"
    );

    // 2. Seed a bash agent via REST.
    let (code, body) = curl_post(
        &format!("{url}/api/agents"),
        r#"{"name":"cli","role":"worker","transport":"local","working_dir":".","project":"cli","agent":"bash"}"#,
    );
    assert!(
        (200..300).contains(&code),
        "POST /api/agents returned {code}: {body}"
    );

    // 3. agents wait --state any-non-starting (the spawn path takes a
    //    beat to flip out of `starting`).
    let (code, _, _) = run_cli(
        &url,
        &[
            "agents",
            "wait",
            "cli",
            "--state",
            "any-non-starting",
            "--timeout",
            "10",
        ],
        None,
    );
    assert_eq!(code, 0, "wait any-non-starting should succeed");

    // 4. agents input cli "echo HELLO_FROM_CLI" + wait idle.
    let (code, _, _) = run_cli(
        &url,
        &["agents", "input", "cli", "echo HELLO_FROM_CLI"],
        None,
    );
    assert_eq!(code, 0, "agents input should succeed");
    let (code, _, _) = run_cli(
        &url,
        &[
            "agents",
            "wait",
            "cli",
            "--state",
            "idle",
            "--timeout",
            "5",
        ],
        None,
    );
    assert_eq!(code, 0, "wait idle after input");

    // 5. agents screenshot — assert the echo output lands.
    let shot_ok = poll_until(Duration::from_secs(5), || {
        let (code, stdout, _) = run_cli(
            &url,
            &["agents", "screenshot", "cli", "--rows", "20", "--cols", "80"],
            None,
        );
        code == 0 && stdout.contains("HELLO_FROM_CLI")
    });
    assert!(shot_ok, "screenshot never showed HELLO_FROM_CLI");

    // 6. agents state cli — sanity check exit 0 + parses.
    let (code, stdout, _) = run_cli(&url, &["agents", "state", "cli", "--format", "json"], None);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("\"id\": \"cli\""),
        "state output: {stdout:?}"
    );

    // 7. agents state unknown → exit 2.
    let (code, _, stderr) = run_cli(&url, &["agents", "state", "ghost"], None);
    assert_eq!(code, 2, "unknown agent should exit 2");
    assert!(stderr.contains("not found"), "stderr: {stderr:?}");

    // 8. events send + list round-trip. Send targets the cli agent
    //    itself so the router has somewhere to deliver.
    let (code, _, _) = run_cli(
        &url,
        &[
            "events",
            "send",
            "--from",
            "cli",
            "--to",
            "cli",
            "--kind",
            "completion_notice",
            "--summary",
            "atn-cli test round-trip",
        ],
        None,
    );
    assert_eq!(code, 0, "events send");
    // Router polls every ~2s; poll for the entry to surface.
    let listed = poll_until(Duration::from_secs(10), || {
        let (code, stdout, _) =
            run_cli(&url, &["events", "list", "--format", "json"], None);
        code == 0 && stdout.contains("atn-cli test round-trip")
    });
    assert!(listed, "sent event never surfaced in events list");

    // 9. Bad kind → exit 1 with the valid-values hint.
    let (code, _, stderr) = run_cli(
        &url,
        &[
            "events",
            "send",
            "--from",
            "cli",
            "--kind",
            "nope",
            "--summary",
            "x",
        ],
        None,
    );
    assert_eq!(code, 1, "bad --kind should exit 1");
    assert!(
        stderr.contains("valid values"),
        "stderr: {stderr:?}"
    );

    // 10. wiki get on a seeded coordination page.
    let (code, stdout, _) = run_cli(&url, &["wiki", "get", "Coordination/Goals"], None);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Goals") || stdout.contains("objectives"),
        "wiki get stdout: {stdout:?}"
    );

    // 11. wiki put + delete round-trip. Capture the ETag via
    //     `--verbose wiki get` on stderr.
    let (code, _, stderr) = run_cli(
        &url,
        &["--verbose", "wiki", "get", "Coordination/Goals"],
        None,
    );
    assert_eq!(code, 0);
    let etag = stderr
        .lines()
        .find_map(|l| l.strip_prefix("ETag: "))
        .expect("ETag on stderr");
    let etag = etag.trim().to_string();
    assert!(etag.starts_with('"'), "etag shape: {etag:?}");

    // Put new content with the valid ETag.
    let (code, _, _) = run_cli(
        &url,
        &[
            "wiki",
            "put",
            "Coordination/Goals",
            "--stdin",
            "--if-match",
            &etag,
        ],
        Some("# Goals (updated via atn-cli test)\n"),
    );
    assert_eq!(code, 0, "put with valid etag");

    // Stale etag → exit 2 with mismatch message.
    let (code, _, stderr) = run_cli(
        &url,
        &[
            "wiki",
            "put",
            "Coordination/Goals",
            "--stdin",
            "--if-match",
            "\"stale-etag-0000\"",
        ],
        Some("# stale write\n"),
    );
    assert_eq!(code, 2, "stale etag should exit 2");
    assert!(
        stderr.contains("ETag mismatch"),
        "stderr: {stderr:?}"
    );

    // 12. wiki put/delete on a brand-new page — no --if-match needed
    //     to create, ETag required for delete.
    let (code, _, _) = run_cli(
        &url,
        &["wiki", "put", "Scratch/AtnCliTest", "--stdin"],
        Some("hello scratch\n"),
    );
    assert_eq!(code, 0, "create scratch page");
    let (code, _, stderr) = run_cli(
        &url,
        &["--verbose", "wiki", "get", "Scratch/AtnCliTest"],
        None,
    );
    assert_eq!(code, 0);
    let scratch_etag = stderr
        .lines()
        .find_map(|l| l.strip_prefix("ETag: "))
        .expect("scratch ETag")
        .trim()
        .to_string();
    let (code, _, _) = run_cli(
        &url,
        &[
            "wiki",
            "delete",
            "Scratch/AtnCliTest",
            "--if-match",
            &scratch_etag,
        ],
        None,
    );
    assert_eq!(code, 0, "delete with valid etag");
    let (code, _, _) = run_cli(&url, &["wiki", "get", "Scratch/AtnCliTest"], None);
    assert_eq!(code, 2, "get after delete should exit 2");
}
