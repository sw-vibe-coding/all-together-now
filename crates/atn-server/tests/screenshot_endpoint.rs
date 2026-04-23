//! Integration tests for `GET /api/agents/{id}/screenshot` (ops-polish
//! saga step 3).
//!
//! Boots `atn-server` in a temp directory with `tools/` on PATH, POSTs
//! a single `fake-claude` agent, waits for its banner to land in the
//! transcript, then exercises every response path: text (default),
//! ansi (raw SGR), html (self-contained `<pre>`), and the two error
//! modes (bad format → 400, unknown id → 404).

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
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
            "[project]\nname = \"screenshot-test\"\nlog_dir = \".atn/logs\"\n",
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

/// One-shot curl. Returns (status_code, body, content_type).
fn curl(method: &str, url: &str, body: Option<&Value>) -> (u16, String, String) {
    let mut cmd = Command::new("curl");
    cmd.args([
        "-sS",
        "-o",
        "-",
        "-w",
        "\n__STATUS__=%{http_code}\n__CTYPE__=%{content_type}",
        "-X",
        method,
    ]);
    if let Some(b) = body {
        cmd.args(["-H", "Content-Type: application/json"]);
        cmd.args(["--data-binary", &b.to_string()]);
    }
    cmd.arg(url);
    let out = cmd.output().expect("curl invocation");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let (rest, ctype) = stdout
        .rsplit_once("\n__CTYPE__=")
        .unwrap_or((stdout.as_str(), ""));
    let (body_part, status) = rest
        .rsplit_once("__STATUS__=")
        .unwrap_or((rest, "0"));
    let body_part = body_part.trim_end_matches('\n').to_string();
    (
        status.trim().parse().unwrap_or(0),
        body_part,
        ctype.trim().to_string(),
    )
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
fn screenshot_endpoint_covers_every_response_path() {
    let srv = ServerGuard::boot();

    // Spawn one fake-claude agent. Its banner is deterministic.
    let spec = json!({
        "name": "shot",
        "role": "coordinator",
        "transport": "local",
        "working_dir": ".",
        "project": "shot",
        "agent": "fake-claude",
    });
    let (code, body, _) = curl("POST", &srv.url("/api/agents"), Some(&spec));
    assert!(
        (200..300).contains(&code),
        "POST /api/agents returned {code}: {body}"
    );

    // Wait for the banner to land in the screenshot. fake-claude prints
    // `fake-claude: coordinator is up` on startup.
    let url = srv.url("/api/agents/shot/screenshot?format=text&rows=20&cols=80");
    let got_banner = poll_until(Duration::from_secs(5), || {
        let (c, b, _) = curl("GET", &url, None);
        c == 200 && b.contains("fake-claude: coordinator is up")
    });
    assert!(got_banner, "banner never appeared in the screenshot");

    // text/plain (default + explicit).
    let (code, body, ctype) = curl("GET", &srv.url("/api/agents/shot/screenshot"), None);
    assert_eq!(code, 200);
    assert!(ctype.starts_with("text/plain"), "ctype: {ctype:?}");
    assert!(
        body.contains("fake-claude"),
        "default text screenshot missing banner; got: {body}"
    );

    // html with custom geometry.
    let (code, body, ctype) = curl(
        "GET",
        &srv.url("/api/agents/shot/screenshot?format=html&rows=10&cols=60"),
        None,
    );
    assert_eq!(code, 200);
    assert!(ctype.starts_with("text/html"), "ctype: {ctype:?}");
    assert!(body.starts_with("<pre"), "html body: {body:?}");
    assert!(body.contains("fake-claude"), "html body missing banner");

    // ansi (still text/plain with raw SGR bytes).
    let (code, _body, ctype) = curl(
        "GET",
        &srv.url("/api/agents/shot/screenshot?format=ansi"),
        None,
    );
    assert_eq!(code, 200);
    assert!(ctype.starts_with("text/plain"), "ctype: {ctype:?}");

    // Bad format → 400.
    let (code, body, _) = curl(
        "GET",
        &srv.url("/api/agents/shot/screenshot?format=bogus"),
        None,
    );
    assert_eq!(code, 400, "bad-format body: {body}");
    assert!(body.contains("unknown format"), "400 body: {body}");

    // Unknown agent → 404.
    let (code, body, _) = curl("GET", &srv.url("/api/agents/nope/screenshot"), None);
    assert_eq!(code, 404, "404 body: {body}");
    assert!(body.contains("not found"), "404 body: {body}");
}
