//! End-to-end test of the three-agent demo topology from an empty-start server.
//!
//! Spawns the real `atn-server` binary on an OS-picked free port with a
//! tempdir cwd and `tools/` prepended to PATH so the fake agent shims
//! (`fake-claude`, `fake-codex`, `fake-opencode-glm5`) resolve. POSTs three
//! `SpawnSpec` payloads, polls `/api/agents` until all three reach a
//! non-`starting` state, routes an event from the coordinator to
//! `worker-hlasm`, and asserts the message router delivered it into the
//! worker's inbox.
//!
//! The fixtures used here are the CI variant (all local transport, fake
//! agent CLIs). The on-disk `demos/three-agent/fixtures/*.json` files remain
//! the real topology (mosh to queenbee etc.) for humans running the demo.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Absolute path to the repo root (where `tools/` and `target/` live).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Path to the just-built `atn-server` binary. `cargo test` builds deps first,
/// but `bin` targets aren't auto-built for tests — so we run `cargo build`
/// once if needed.
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
    assert!(bin.exists(), "atn-server binary not found at {bin:?}");
    bin
}

/// RAII guard that kills the server subprocess when the test ends or panics.
struct ServerGuard {
    child: Option<Child>,
    port: u16,
    base_dir: PathBuf,
    _tmp: tempfile::TempDir,
}

impl ServerGuard {
    fn boot() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base_dir = tmp.path().to_path_buf();
        std::fs::write(
            base_dir.join("agents.toml"),
            "[project]\nname = \"three-agent-test\"\n",
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
        // The server prints `atn-server ready on 0.0.0.0:<port>` once bound.
        // Read line-by-line until we see it or the server exits.
        for line in reader.lines().take(200).map_while(Result::ok) {
            if let Some(rest) = line.strip_prefix("atn-server ready on ")
                && let Some((_, p)) = rest.rsplit_once(':')
                && let Ok(parsed) = p.parse::<u16>()
            {
                port = Some(parsed);
                break;
            }
        }
        let port = port.expect("never saw `atn-server ready on ...` on stdout");

        Self {
            child: Some(child),
            port,
            base_dir,
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

/// Shell out to curl for a single HTTP call. Returns (status_code, body).
/// Rust devs, avert thine eyes — reqwest would pull a mountain of deps for
/// a couple of lines of network glue in this one test.
fn curl(method: &str, url: &str, body: Option<&Value>) -> (u16, String) {
    let mut cmd = Command::new("curl");
    cmd.args([
        "-sS",
        "-o",
        "-",
        "-w",
        "\n__STATUS__=%{http_code}",
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
    // Parse the trailing status marker.
    let (body_str, status) = match stdout.rsplit_once("__STATUS__=") {
        Some((b, s)) => (b.trim_end_matches('\n').to_string(), s.trim()),
        None => (stdout, "0"),
    };
    let code = status.parse().unwrap_or(0);
    (code, body_str)
}

fn create_spec(name: &str, role: &str, fake_agent: &str) -> Value {
    json!({
        "name": name,
        "role": role,
        "transport": "local",
        "working_dir": ".",
        "project": name,
        "agent": fake_agent,
    })
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

fn any_entry_is<P: AsRef<Path>>(dir: P) -> bool {
    std::fs::read_dir(&dir)
        .ok()
        .map(|it| it.count() > 0)
        .unwrap_or(false)
}

#[test]
fn three_agent_topology_end_to_end() {
    let srv = ServerGuard::boot();

    // 1. Empty start: GET /api/agents returns [].
    let (code, body) = curl("GET", &srv.url("/api/agents"), None);
    assert_eq!(code, 200, "empty GET failed: {body}");
    assert_eq!(body.trim(), "[]", "expected empty list, got {body}");

    // 2. POST the three fixtures (CI variant: local transport, fake CLIs).
    let fixtures = [
        create_spec("coordinator", "coordinator", "fake-claude"),
        create_spec("worker-hlasm", "worker", "fake-codex"),
        create_spec("worker-rpg", "worker", "fake-opencode-glm5"),
    ];

    for spec in &fixtures {
        let (code, body) = curl("POST", &srv.url("/api/agents"), Some(spec));
        assert_eq!(code, 201, "POST {spec} returned {code}: {body}");
    }

    // 3. Poll /api/agents until all three show up and none are still in
    //    `starting`. (Fake agents echo a banner then block on stdin, which
    //    the state tracker reads as Running/Idle.)
    let saw_all_running = poll_until(Duration::from_secs(10), || {
        let (_, body) = curl("GET", &srv.url("/api/agents"), None);
        let list: Vec<Value> = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if list.len() != 3 {
            return false;
        }
        list.iter().all(|a| {
            a["state"]["state"].as_str() != Some("starting")
                && a["state"]["state"].as_str() != Some("disconnected")
        })
    });
    assert!(
        saw_all_running,
        "agents never all reached a running state; last list = {}",
        curl("GET", &srv.url("/api/agents"), None).1
    );

    // 4. Route two events: coordinator → worker-hlasm and coordinator →
    //    worker-rpg. Both inboxes should receive a delivered message after
    //    the router's next poll.
    let atn_dir = srv.base_dir.join(".atn");
    let inbox_root = atn_dir.join("inboxes");

    for (evt_id, target) in [
        ("test-evt-hlasm", "worker-hlasm"),
        ("test-evt-rpg", "worker-rpg"),
    ] {
        let now = chrono::Utc::now().to_rfc3339();
        let event = json!({
            "id": evt_id,
            "kind": "feature_request",
            "source_agent": "coordinator",
            "source_repo": ".",
            "target_agent": target,
            "issue_id": null,
            "summary": format!("three-agent demo: task for {target}"),
            "wiki_link": null,
            "priority": "normal",
            "timestamp": now,
        });
        let (code, body) = curl("POST", &srv.url("/api/events"), Some(&event));
        assert!(
            (200..300).contains(&code),
            "POST /api/events → {target} returned {code}: {body}"
        );
    }

    // Router polls every 2s. Give it up to 10s to deliver both.
    let hlasm_inbox = inbox_root.join("worker-hlasm");
    let rpg_inbox = inbox_root.join("worker-rpg");
    let both_delivered = poll_until(Duration::from_secs(10), || {
        any_entry_is(&hlasm_inbox) && any_entry_is(&rpg_inbox)
    });
    assert!(
        both_delivered,
        "events never reached both inboxes; hlasm={} rpg={}",
        any_entry_is(&hlasm_inbox),
        any_entry_is(&rpg_inbox)
    );

    // 5. Event log surfaces the delivered events (serves the UI's events
    //    tab). Expect at least two entries.
    let (_, body) = curl("GET", &srv.url("/api/events"), None);
    let entries: Vec<Value> = serde_json::from_str(&body).unwrap_or_default();
    assert!(
        entries.len() >= 2,
        "event log should contain the two events we posted; got {}: {body}",
        entries.len()
    );
}
