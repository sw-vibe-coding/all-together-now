//! atn-cli — typed HTTP client for the ATN server.
//!
//! Step 1 (cli-scaffold): base-URL resolution + `agents list` +
//! `agents state <id>`. Subsequent saga steps add input/stop/restart/
//! wait/screenshot, events, and wiki subcommands.
//!
//! # Exit codes
//!
//! - `0` success
//! - `1` usage error (bad args)
//! - `2` not found (404 from the server or unknown agent/wiki page)
//! - `3` http / transport error
//! - `4` server error (5xx)
//!
//! # Base URL resolution
//!
//! `--base-url <url>` → `ATN_URL` env var → `http://localhost:7500`.

use std::io::Write;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Deserialize;
use serde_json::Value;

const DEFAULT_BASE_URL: &str = "http://localhost:7500";

const EXIT_OK: u8 = 0;
// EXIT_USAGE (1) is reserved for clap's own parser-error exit; this
// binary never constructs it explicitly. Kept here as documentation.
#[allow(dead_code)]
const EXIT_USAGE: u8 = 1;
const EXIT_NOT_FOUND: u8 = 2;
const EXIT_HTTP: u8 = 3;
const EXIT_SERVER: u8 = 4;

#[derive(Parser)]
#[command(
    name = "atn-cli",
    version,
    about = "Typed CLI for the ATN HTTP API",
    long_about = "Typed CLI for the ATN HTTP API.\n\n\
                  Base URL resolution: --base-url, then ATN_URL env, \
                  then http://localhost:7500.\n\n\
                  Exit codes:\n  \
                  0 = ok\n  \
                  1 = usage error\n  \
                  2 = not found (404)\n  \
                  3 = http / transport error\n  \
                  4 = server error (5xx)"
)]
struct Cli {
    /// Override the server base URL.
    #[arg(long, global = true, value_name = "URL")]
    base_url: Option<String>,

    /// Print the resolved base URL and request URL before each call.
    #[arg(long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Agent lifecycle + observation commands.
    Agents {
        #[command(subcommand)]
        action: AgentsCommand,
    },
    /// Inter-agent event log — list + send.
    Events {
        #[command(subcommand)]
        action: EventsCommand,
    },
}

#[derive(Subcommand)]
enum EventsCommand {
    /// List event-log entries (optionally since index N).
    List {
        /// Start index (exclusive of everything before).
        #[arg(long)]
        since: Option<usize>,
        #[command(flatten)]
        fmt: FormatArg,
    },
    /// Submit a PushEvent to the message router.
    ///
    /// Valid kinds: feature_request, bug_fix_request,
    /// completion_notice, blocked_notice, needs_info,
    /// verification_request.
    Send {
        /// Source agent id.
        #[arg(long)]
        from: String,
        /// Target agent id (omit to broadcast / escalate).
        #[arg(long)]
        to: Option<String>,
        /// Event kind (see subcommand help for valid values).
        #[arg(long)]
        kind: String,
        /// Human-readable summary that becomes the agent's prompt.
        #[arg(long)]
        summary: String,
        /// Priority — one of `normal`, `high`, `blocking`.
        #[arg(long, default_value = "normal")]
        priority: String,
        /// Optional issue tracker id.
        #[arg(long)]
        issue_id: Option<String>,
        /// Optional wiki path (e.g. `Coordination/Requests`).
        #[arg(long)]
        wiki_link: Option<String>,
        /// Optional repo label for the source.
        #[arg(long, default_value = ".")]
        source_repo: String,
    },
}

#[derive(Subcommand)]
enum AgentsCommand {
    /// List all agents with state + stalled flag.
    List(ListArgs),
    /// Show a single agent's full state snapshot.
    State {
        /// Agent id.
        id: String,
        #[command(flatten)]
        fmt: FormatArg,
    },
    /// Send text input to an agent's PTY (auto-appends `\r`).
    Input {
        /// Agent id.
        id: String,
        /// Text to send. Omit with `--stdin` to read from stdin instead.
        text: Option<String>,
        /// Read the prompt from stdin instead of the positional arg.
        #[arg(long)]
        stdin: bool,
    },
    /// POST /api/agents/{id}/stop.
    Stop {
        /// Agent id.
        id: String,
    },
    /// POST /api/agents/{id}/restart (graceful Ctrl-C + respawn).
    Restart {
        /// Agent id.
        id: String,
    },
    /// POST /api/agents/{id}/reconnect (hard-kill local mosh/ssh + respawn).
    Reconnect {
        /// Agent id.
        id: String,
    },
    /// DELETE /api/agents/{id}.
    Delete {
        /// Agent id.
        id: String,
    },
    /// Poll an agent's state until it matches (or timeout).
    ///
    /// Canonical state strings: starting, running, idle,
    /// awaiting_human_input, busy, blocked, completed_task, error,
    /// disconnected. Plus the umbrella `any-non-starting`.
    Wait {
        /// Agent id.
        id: String,
        /// State to wait for. Default: `idle`.
        #[arg(long, default_value = "idle")]
        state: String,
        /// Max seconds to wait before exiting non-zero.
        #[arg(long, default_value_t = 30)]
        timeout: u64,
        /// Initial poll interval in milliseconds. Doubles each retry
        /// (capped at 4× this value) for light exponential backoff.
        #[arg(long, default_value_t = 500)]
        poll_interval: u64,
    },
    /// Fetch a rendered terminal snapshot and print it to stdout.
    Screenshot {
        /// Agent id.
        id: String,
        /// Output format — passed through to the server endpoint.
        #[arg(long, value_enum, default_value_t = ScreenshotFormat::Text)]
        format: ScreenshotFormat,
        /// Virtual terminal rows.
        #[arg(long, default_value_t = 40)]
        rows: u32,
        /// Virtual terminal cols.
        #[arg(long, default_value_t = 120)]
        cols: u32,
    },
}

#[derive(Copy, Clone, ValueEnum, Debug)]
enum ScreenshotFormat {
    Text,
    Ansi,
    Html,
}

impl ScreenshotFormat {
    fn as_str(self) -> &'static str {
        match self {
            ScreenshotFormat::Text => "text",
            ScreenshotFormat::Ansi => "ansi",
            ScreenshotFormat::Html => "html",
        }
    }
}

#[derive(Args)]
struct ListArgs {
    #[command(flatten)]
    fmt: FormatArg,
}

#[derive(Args)]
struct FormatArg {
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    format: OutputFormat,
}

#[derive(Copy, Clone, ValueEnum, PartialEq, Eq, Debug)]
enum OutputFormat {
    Table,
    Json,
}

fn resolve_base_url(flag: Option<String>) -> String {
    flag.or_else(|| std::env::var("ATN_URL").ok())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
}

/// Minimal agent-info shape — matches the server's `AgentInfo` JSON
/// subset we care about here.
#[derive(Deserialize, Debug)]
struct AgentInfo {
    id: String,
    name: String,
    role: String,
    state: Value,
    #[serde(default)]
    stalled: bool,
    #[serde(default)]
    stalled_for_secs: Option<u64>,
}

fn state_label(state: &Value) -> String {
    // AgentState serializes as {"state": "<snake>", ...maybe fields}.
    match state.get("state").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => state.to_string(),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let base = resolve_base_url(cli.base_url);
    if cli.verbose {
        let _ = writeln!(std::io::stderr(), "atn-cli: base_url = {base}");
    }
    let code = match cli.command {
        Command::Agents { action } => run_agents(&base, cli.verbose, action),
        Command::Events { action } => run_events(&base, cli.verbose, action),
    };
    ExitCode::from(code)
}

fn run_agents(base: &str, verbose: bool, cmd: AgentsCommand) -> u8 {
    match cmd {
        AgentsCommand::List(ListArgs { fmt }) => agents_list(base, verbose, fmt.format),
        AgentsCommand::State { id, fmt } => agents_state(base, verbose, &id, fmt.format),
        AgentsCommand::Input { id, text, stdin } => agents_input(base, verbose, &id, text, stdin),
        AgentsCommand::Stop { id } => agents_post_action(base, verbose, &id, "stop"),
        AgentsCommand::Restart { id } => agents_post_action(base, verbose, &id, "restart"),
        AgentsCommand::Reconnect { id } => agents_post_action(base, verbose, &id, "reconnect"),
        AgentsCommand::Delete { id } => agents_delete(base, verbose, &id),
        AgentsCommand::Wait {
            id,
            state,
            timeout,
            poll_interval,
        } => agents_wait(base, verbose, &id, &state, timeout, poll_interval),
        AgentsCommand::Screenshot {
            id,
            format,
            rows,
            cols,
        } => agents_screenshot(base, verbose, &id, format, rows, cols),
    }
}

fn agents_input(base: &str, verbose: bool, id: &str, text: Option<String>, stdin: bool) -> u8 {
    let body = if stdin {
        let mut buf = String::new();
        if let Err(e) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf) {
            let _ = writeln!(std::io::stderr(), "failed to read stdin: {e}");
            return EXIT_HTTP;
        }
        buf
    } else if let Some(t) = text {
        t
    } else {
        let _ = writeln!(
            std::io::stderr(),
            "error: pass text as a positional arg or use --stdin"
        );
        return EXIT_USAGE;
    };
    // Match the UI's atomic text+Enter send — append `\r` so the PTY
    // writes the whole line in one input event. Multi-line input
    // (--stdin) ends with `\r` exactly once to commit the final line.
    let mut payload = body;
    if !payload.ends_with('\r') && !payload.ends_with('\n') {
        payload.push('\r');
    }
    let url = format!("{base}/api/agents/{id}/input");
    if verbose {
        let _ = writeln!(std::io::stderr(), "POST {url} ({} bytes)", payload.len());
    }
    let json = serde_json::json!({ "text": payload });
    match ureq::post(&url).send_json(json) {
        Ok(_) => EXIT_OK,
        Err(ureq::Error::Status(404, _)) => {
            let _ = writeln!(std::io::stderr(), "agent '{id}' not found");
            EXIT_NOT_FOUND
        }
        Err(e) => report_http_error(&url, e),
    }
}

fn agents_post_action(base: &str, verbose: bool, id: &str, action: &str) -> u8 {
    let url = format!("{base}/api/agents/{id}/{action}");
    if verbose {
        let _ = writeln!(std::io::stderr(), "POST {url}");
    }
    match ureq::post(&url).call() {
        Ok(_) => EXIT_OK,
        Err(ureq::Error::Status(404, _)) => {
            let _ = writeln!(std::io::stderr(), "agent '{id}' not found");
            EXIT_NOT_FOUND
        }
        Err(e) => report_http_error(&url, e),
    }
}

fn agents_delete(base: &str, verbose: bool, id: &str) -> u8 {
    let url = format!("{base}/api/agents/{id}");
    if verbose {
        let _ = writeln!(std::io::stderr(), "DELETE {url}");
    }
    match ureq::delete(&url).call() {
        Ok(_) => EXIT_OK,
        Err(ureq::Error::Status(404, _)) => {
            let _ = writeln!(std::io::stderr(), "agent '{id}' not found");
            EXIT_NOT_FOUND
        }
        Err(e) => report_http_error(&url, e),
    }
}

fn agents_wait(
    base: &str,
    verbose: bool,
    id: &str,
    state: &str,
    timeout: u64,
    poll_interval: u64,
) -> u8 {
    let want = StateMatch::parse(state);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    let mut backoff_ms = poll_interval;
    let cap_ms = poll_interval.saturating_mul(4);
    let url = format!("{base}/api/agents/{id}/state");
    loop {
        if verbose {
            let _ = writeln!(std::io::stderr(), "GET {url} (backoff_ms = {backoff_ms})");
        }
        match ureq::get(&url).call() {
            Ok(resp) => {
                let body = resp.into_string().unwrap_or_default();
                if let Ok(ai) = serde_json::from_str::<AgentInfo>(&body)
                    && want.matches(&state_label(&ai.state))
                {
                    return EXIT_OK;
                }
            }
            Err(ureq::Error::Status(404, _)) => {
                let _ = writeln!(std::io::stderr(), "agent '{id}' not found");
                return EXIT_NOT_FOUND;
            }
            Err(ureq::Error::Transport(_)) => {
                // Transient — try again after backoff until deadline.
            }
            Err(e) => return report_http_error(&url, e),
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            let _ = writeln!(
                std::io::stderr(),
                "timeout waiting for agent '{id}' to reach state {state:?}"
            );
            return EXIT_HTTP;
        }
        let remaining = deadline.saturating_duration_since(now);
        let sleep = std::time::Duration::from_millis(backoff_ms).min(remaining);
        std::thread::sleep(sleep);
        backoff_ms = (backoff_ms.saturating_mul(2)).min(cap_ms);
    }
}

fn agents_screenshot(
    base: &str,
    verbose: bool,
    id: &str,
    format: ScreenshotFormat,
    rows: u32,
    cols: u32,
) -> u8 {
    let url = format!(
        "{base}/api/agents/{id}/screenshot?format={fmt}&rows={rows}&cols={cols}",
        fmt = format.as_str(),
    );
    if verbose {
        let _ = writeln!(std::io::stderr(), "GET {url}");
    }
    let resp = match ureq::get(&url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(404, _)) => {
            let _ = writeln!(std::io::stderr(), "agent '{id}' not found");
            return EXIT_NOT_FOUND;
        }
        Err(e) => return report_http_error(&url, e),
    };
    let status = resp.status();
    let body = resp.into_string().unwrap_or_default();
    if !(200..300).contains(&status) {
        return report_status_error(&url, status, &body);
    }
    print!("{body}");
    EXIT_OK
}

/// Parsed form of the `--state` argument.
#[derive(Debug, PartialEq, Eq)]
enum StateMatch {
    /// Exact match against `AgentState::state` (canonical snake_case).
    Exact(String),
    /// Any state that isn't `starting` — matches `agents wait` use in
    /// scripts that just want the PTY to have warmed up.
    AnyNonStarting,
}

impl StateMatch {
    fn parse(s: &str) -> Self {
        match s {
            "any-non-starting" | "any_non_starting" => StateMatch::AnyNonStarting,
            // Accept hyphenated and snake-cased spellings from CLI flags.
            "awaiting-input" | "awaiting-human-input" => {
                StateMatch::Exact("awaiting_human_input".into())
            }
            "completed-task" => StateMatch::Exact("completed_task".into()),
            other => StateMatch::Exact(other.replace('-', "_")),
        }
    }

    fn matches(&self, actual: &str) -> bool {
        match self {
            StateMatch::Exact(want) => actual == want,
            StateMatch::AnyNonStarting => actual != "starting",
        }
    }
}

fn agents_list(base: &str, verbose: bool, format: OutputFormat) -> u8 {
    let url = format!("{base}/api/agents");
    if verbose {
        let _ = writeln!(std::io::stderr(), "GET {url}");
    }
    let resp = match ureq::get(&url).call() {
        Ok(r) => r,
        Err(e) => return report_http_error(&url, e),
    };
    let status = resp.status();
    let body = match resp.into_string() {
        Ok(s) => s,
        Err(e) => return report_transport_error(&url, e),
    };
    if status == 404 {
        let _ = writeln!(std::io::stderr(), "endpoint {url} returned 404");
        return EXIT_NOT_FOUND;
    }
    if !(200..300).contains(&status) {
        return report_status_error(&url, status, &body);
    }

    match format {
        OutputFormat::Json => {
            // Pretty-print for humans + diagnostics.
            match serde_json::from_str::<Value>(&body) {
                Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap_or(body)),
                Err(_) => println!("{body}"),
            }
            EXIT_OK
        }
        OutputFormat::Table => {
            let agents: Vec<AgentInfo> = match serde_json::from_str(&body) {
                Ok(a) => a,
                Err(e) => {
                    let _ = writeln!(std::io::stderr(), "failed to parse agents: {e}");
                    return EXIT_HTTP;
                }
            };
            print_agents_table(&agents);
            EXIT_OK
        }
    }
}

fn agents_state(base: &str, verbose: bool, id: &str, format: OutputFormat) -> u8 {
    let url = format!("{base}/api/agents/{id}/state");
    if verbose {
        let _ = writeln!(std::io::stderr(), "GET {url}");
    }
    let resp = match ureq::get(&url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(404, _)) => {
            let _ = writeln!(std::io::stderr(), "agent '{id}' not found");
            return EXIT_NOT_FOUND;
        }
        Err(e) => return report_http_error(&url, e),
    };
    let status = resp.status();
    let body = resp.into_string().unwrap_or_default();
    if !(200..300).contains(&status) {
        return report_status_error(&url, status, &body);
    }
    match format {
        OutputFormat::Json => match serde_json::from_str::<Value>(&body) {
            Ok(v) => {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or(body));
                EXIT_OK
            }
            Err(_) => {
                println!("{body}");
                EXIT_OK
            }
        },
        OutputFormat::Table => {
            let agent: AgentInfo = match serde_json::from_str(&body) {
                Ok(a) => a,
                Err(e) => {
                    let _ = writeln!(std::io::stderr(), "failed to parse state: {e}");
                    return EXIT_HTTP;
                }
            };
            print_agents_table(&[agent]);
            EXIT_OK
        }
    }
}

/// Fixed-width table formatter. Pure function (no stdout writes) so it
/// can be unit-tested; main wrappers `println!` the result.
fn format_agents_table(agents: &[AgentInfo]) -> String {
    let headers = ["ID", "NAME", "ROLE", "STATE", "STALLED"];
    let rows: Vec<[String; 5]> = agents
        .iter()
        .map(|a| {
            let stalled = if a.stalled {
                match a.stalled_for_secs {
                    Some(n) => format!("yes ({n}s)"),
                    None => "yes".to_string(),
                }
            } else {
                "-".to_string()
            };
            [
                a.id.clone(),
                a.name.clone(),
                a.role.clone(),
                state_label(&a.state),
                stalled,
            ]
        })
        .collect();
    let mut widths = [0usize; 5];
    for (i, h) in headers.iter().enumerate() {
        widths[i] = h.len();
    }
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.len());
        }
    }
    let mut out = String::new();
    for (i, h) in headers.iter().enumerate() {
        if i > 0 {
            out.push_str("  ");
        }
        out.push_str(&format!("{:<width$}", h, width = widths[i]));
    }
    out.push('\n');
    // Dashed rule under the header.
    for (i, w) in widths.iter().enumerate() {
        if i > 0 {
            out.push_str("  ");
        }
        out.push_str(&"-".repeat(*w));
    }
    out.push('\n');
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                out.push_str("  ");
            }
            out.push_str(&format!("{:<width$}", cell, width = widths[i]));
        }
        out.push('\n');
    }
    out
}

fn print_agents_table(agents: &[AgentInfo]) {
    if agents.is_empty() {
        println!("(no agents)");
        return;
    }
    let out = format_agents_table(agents);
    print!("{out}");
}

fn report_http_error(url: &str, err: ureq::Error) -> u8 {
    match err {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            report_status_error(url, code, &body)
        }
        ureq::Error::Transport(t) => {
            let _ = writeln!(std::io::stderr(), "transport error calling {url}: {t}");
            EXIT_HTTP
        }
    }
}

fn report_transport_error<E: std::fmt::Display>(url: &str, err: E) -> u8 {
    let _ = writeln!(std::io::stderr(), "transport error reading {url}: {err}");
    EXIT_HTTP
}

fn report_status_error(url: &str, status: u16, body: &str) -> u8 {
    let _ = writeln!(std::io::stderr(), "{url} returned {status}: {body}");
    if status == 404 {
        EXIT_NOT_FOUND
    } else if (500..600).contains(&status) {
        EXIT_SERVER
    } else if !(200..300).contains(&status) {
        // Any other non-2xx — treat like http error. CLI tools leaning
        // on exit codes can differentiate by the stderr payload.
        EXIT_HTTP
    } else {
        EXIT_OK
    }
}

// ── Events subcommands ───────────────────────────────────────────────

const VALID_PUSH_KINDS: &[&str] = &[
    "feature_request",
    "bug_fix_request",
    "completion_notice",
    "blocked_notice",
    "needs_info",
    "verification_request",
];

const VALID_PRIORITIES: &[&str] = &["normal", "high", "blocking"];

fn validate_kind(k: &str) -> Result<&'static str, String> {
    // Accept hyphenated aliases too.
    let norm = k.replace('-', "_");
    for canonical in VALID_PUSH_KINDS {
        if canonical == &norm {
            return Ok(canonical);
        }
    }
    Err(format!(
        "invalid kind '{k}'; valid values: {}",
        VALID_PUSH_KINDS.join(", ")
    ))
}

fn validate_priority(p: &str) -> Result<&'static str, String> {
    for canonical in VALID_PRIORITIES {
        if canonical == &p {
            return Ok(canonical);
        }
    }
    Err(format!(
        "invalid priority '{p}'; valid values: {}",
        VALID_PRIORITIES.join(", ")
    ))
}

#[derive(Deserialize)]
struct EventLogEntryLite {
    event: PushEventLite,
    #[serde(default)]
    decision: String,
    #[serde(default)]
    delivered: bool,
    #[serde(default)]
    logged_at: String,
}

#[derive(Deserialize)]
struct PushEventLite {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    source_agent: String,
    #[serde(default)]
    target_agent: Option<String>,
    #[serde(default)]
    summary: String,
}

fn run_events(base: &str, verbose: bool, cmd: EventsCommand) -> u8 {
    match cmd {
        EventsCommand::List { since, fmt } => events_list(base, verbose, since, fmt.format),
        EventsCommand::Send {
            from,
            to,
            kind,
            summary,
            priority,
            issue_id,
            wiki_link,
            source_repo,
        } => events_send(
            base, verbose, &from, to, &kind, &summary, &priority, issue_id, wiki_link, &source_repo,
        ),
    }
}

fn events_list(base: &str, verbose: bool, since: Option<usize>, format: OutputFormat) -> u8 {
    let url = match since {
        Some(n) => format!("{base}/api/events?since={n}"),
        None => format!("{base}/api/events"),
    };
    if verbose {
        let _ = writeln!(std::io::stderr(), "GET {url}");
    }
    let resp = match ureq::get(&url).call() {
        Ok(r) => r,
        Err(e) => return report_http_error(&url, e),
    };
    let status = resp.status();
    let body = resp.into_string().unwrap_or_default();
    if !(200..300).contains(&status) {
        return report_status_error(&url, status, &body);
    }
    match format {
        OutputFormat::Json => {
            match serde_json::from_str::<Value>(&body) {
                Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap_or(body)),
                Err(_) => println!("{body}"),
            }
            EXIT_OK
        }
        OutputFormat::Table => {
            let entries: Vec<EventLogEntryLite> = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(e) => {
                    let _ = writeln!(std::io::stderr(), "failed to parse events: {e}");
                    return EXIT_HTTP;
                }
            };
            print_events_table(&entries);
            EXIT_OK
        }
    }
}

fn format_events_table(entries: &[EventLogEntryLite]) -> String {
    let headers = ["LOGGED_AT", "KIND", "FROM → TO", "DECISION", "DELIVERED", "SUMMARY"];
    let rows: Vec<[String; 6]> = entries
        .iter()
        .map(|e| {
            let route = match &e.event.target_agent {
                Some(to) => format!("{} → {}", e.event.source_agent, to),
                None => format!("{} → broadcast", e.event.source_agent),
            };
            let delivered = if e.delivered { "yes" } else { "no" };
            let summary = if e.event.summary.len() > 80 {
                format!("{}…", &e.event.summary[..79])
            } else {
                e.event.summary.clone()
            };
            [
                e.logged_at.clone(),
                e.event.kind.clone(),
                route,
                e.decision.clone(),
                delivered.to_string(),
                summary,
            ]
        })
        .collect();
    let mut widths = [0usize; 6];
    for (i, h) in headers.iter().enumerate() {
        widths[i] = h.len();
    }
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let mut out = String::new();
    for (i, h) in headers.iter().enumerate() {
        if i > 0 {
            out.push_str("  ");
        }
        out.push_str(&format!("{:<width$}", h, width = widths[i]));
    }
    out.push('\n');
    for (i, w) in widths.iter().enumerate() {
        if i > 0 {
            out.push_str("  ");
        }
        out.push_str(&"-".repeat(*w));
    }
    out.push('\n');
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                out.push_str("  ");
            }
            out.push_str(&format!("{:<width$}", cell, width = widths[i]));
        }
        out.push('\n');
    }
    out
}

fn print_events_table(entries: &[EventLogEntryLite]) {
    if entries.is_empty() {
        println!("(no events)");
        return;
    }
    print!("{}", format_events_table(entries));
}

#[allow(clippy::too_many_arguments)]
fn events_send(
    base: &str,
    verbose: bool,
    from: &str,
    to: Option<String>,
    kind: &str,
    summary: &str,
    priority: &str,
    issue_id: Option<String>,
    wiki_link: Option<String>,
    source_repo: &str,
) -> u8 {
    let canonical_kind = match validate_kind(kind) {
        Ok(k) => k,
        Err(msg) => {
            let _ = writeln!(std::io::stderr(), "{msg}");
            return EXIT_USAGE;
        }
    };
    let canonical_priority = match validate_priority(priority) {
        Ok(p) => p,
        Err(msg) => {
            let _ = writeln!(std::io::stderr(), "{msg}");
            return EXIT_USAGE;
        }
    };
    let event = build_push_event(
        from,
        to,
        canonical_kind,
        summary,
        canonical_priority,
        issue_id,
        wiki_link,
        source_repo,
    );
    let url = format!("{base}/api/events");
    if verbose {
        let _ = writeln!(std::io::stderr(), "POST {url}");
    }
    match ureq::post(&url).send_json(event) {
        Ok(_) => EXIT_OK,
        Err(e) => report_http_error(&url, e),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_push_event(
    from: &str,
    to: Option<String>,
    kind: &str,
    summary: &str,
    priority: &str,
    issue_id: Option<String>,
    wiki_link: Option<String>,
    source_repo: &str,
) -> Value {
    let id = format!(
        "cli-{from}-{millis}",
        millis = chrono::Utc::now().timestamp_millis()
    );
    let timestamp = chrono::Utc::now().to_rfc3339();
    serde_json::json!({
        "id": id,
        "kind": kind,
        "source_agent": from,
        "source_repo": source_repo,
        "target_agent": to,
        "issue_id": issue_id,
        "summary": summary,
        "wiki_link": wiki_link,
        "priority": priority,
        "timestamp": timestamp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn base_url_precedence_flag_wins() {
        // Clear any external env bleed for this single assertion.
        let saved = std::env::var("ATN_URL").ok();
        // SAFETY: tests run serially by default for env mutation here.
        unsafe {
            std::env::set_var("ATN_URL", "http://from-env:7500");
        }
        assert_eq!(
            resolve_base_url(Some("http://from-flag:7500".to_string())),
            "http://from-flag:7500"
        );
        unsafe {
            match saved {
                Some(v) => std::env::set_var("ATN_URL", v),
                None => std::env::remove_var("ATN_URL"),
            }
        }
    }

    #[test]
    fn base_url_env_wins_over_default() {
        let saved = std::env::var("ATN_URL").ok();
        unsafe {
            std::env::set_var("ATN_URL", "http://envwin:9999");
        }
        assert_eq!(resolve_base_url(None), "http://envwin:9999");
        unsafe {
            match saved {
                Some(v) => std::env::set_var("ATN_URL", v),
                None => std::env::remove_var("ATN_URL"),
            }
        }
    }

    #[test]
    fn base_url_default_when_nothing_set() {
        let saved = std::env::var("ATN_URL").ok();
        unsafe {
            std::env::remove_var("ATN_URL");
        }
        assert_eq!(resolve_base_url(None), "http://localhost:7500");
        unsafe {
            if let Some(v) = saved {
                std::env::set_var("ATN_URL", v);
            }
        }
    }

    #[test]
    fn table_handles_empty_input() {
        // The public print helper prints "(no agents)" for empty; the
        // formatter itself produces just header + rule for no rows.
        let out = format_agents_table(&[]);
        // Two lines: header and dashed rule.
        assert_eq!(out.lines().count(), 2);
        assert!(out.contains("ID"));
    }

    #[test]
    fn table_right_pads_columns() {
        let agents = vec![
            AgentInfo {
                id: "a".into(),
                name: "short".into(),
                role: "coordinator".into(),
                state: json!({"state": "running"}),
                stalled: false,
                stalled_for_secs: None,
            },
            AgentInfo {
                id: "worker-hlasm".into(),
                name: "hlasm (hlasm)".into(),
                role: "developer".into(),
                state: json!({"state": "idle"}),
                stalled: true,
                stalled_for_secs: Some(5),
            },
        ];
        let out = format_agents_table(&agents);
        // Both rows + header + rule.
        assert_eq!(out.lines().count(), 4);
        let lines: Vec<&str> = out.lines().collect();
        // Header width is at least as wide as the longest id.
        assert!(lines[0].starts_with("ID"));
        // Dashed rule has a column for every header.
        assert!(lines[1].contains("--"));
        // stalled=false renders as "-".
        assert!(lines[2].contains("coordinator"));
        assert!(lines[2].contains("running"));
        assert!(lines[2].trim_end().ends_with('-'));
        // stalled=true + seconds renders with "yes (5s)".
        assert!(lines[3].contains("yes (5s)"));
        assert!(lines[3].contains("developer"));
    }

    #[test]
    fn state_match_parses_hyphen_and_snake_forms() {
        assert_eq!(
            StateMatch::parse("awaiting-input"),
            StateMatch::Exact("awaiting_human_input".into())
        );
        assert_eq!(
            StateMatch::parse("awaiting-human-input"),
            StateMatch::Exact("awaiting_human_input".into())
        );
        assert_eq!(
            StateMatch::parse("completed-task"),
            StateMatch::Exact("completed_task".into())
        );
        assert_eq!(
            StateMatch::parse("idle"),
            StateMatch::Exact("idle".into())
        );
        assert_eq!(StateMatch::parse("any-non-starting"), StateMatch::AnyNonStarting);
        assert_eq!(StateMatch::parse("any_non_starting"), StateMatch::AnyNonStarting);
    }

    #[test]
    fn state_match_matches_actual_values() {
        let idle = StateMatch::parse("idle");
        assert!(idle.matches("idle"));
        assert!(!idle.matches("running"));

        let awaiting = StateMatch::parse("awaiting-input");
        assert!(awaiting.matches("awaiting_human_input"));
        assert!(!awaiting.matches("idle"));

        let any = StateMatch::parse("any-non-starting");
        assert!(!any.matches("starting"));
        assert!(any.matches("running"));
        assert!(any.matches("idle"));
        assert!(any.matches("awaiting_human_input"));
        assert!(any.matches("disconnected"));
    }

    #[test]
    fn validate_kind_accepts_canonical_and_hyphen_aliases() {
        assert_eq!(validate_kind("completion_notice").unwrap(), "completion_notice");
        assert_eq!(validate_kind("completion-notice").unwrap(), "completion_notice");
        assert_eq!(validate_kind("bug-fix-request").unwrap(), "bug_fix_request");
        assert_eq!(validate_kind("verification_request").unwrap(), "verification_request");
    }

    #[test]
    fn validate_kind_rejects_unknown() {
        let err = validate_kind("nope").unwrap_err();
        // Error spells out the full list of valid kinds for `--help`-style UX.
        assert!(err.contains("feature_request"));
        assert!(err.contains("verification_request"));
    }

    #[test]
    fn validate_priority_lists_valid_values() {
        assert_eq!(validate_priority("normal").unwrap(), "normal");
        assert_eq!(validate_priority("high").unwrap(), "high");
        assert_eq!(validate_priority("blocking").unwrap(), "blocking");
        let err = validate_priority("urgent").unwrap_err();
        assert!(err.contains("normal"));
        assert!(err.contains("blocking"));
    }

    #[test]
    fn build_push_event_has_required_fields() {
        let ev = build_push_event(
            "worker-hlasm",
            Some("coordinator".to_string()),
            "completion_notice",
            "task X done",
            "high",
            Some("ATN-42".to_string()),
            None,
            ".",
        );
        assert_eq!(ev["source_agent"], "worker-hlasm");
        assert_eq!(ev["target_agent"], "coordinator");
        assert_eq!(ev["kind"], "completion_notice");
        assert_eq!(ev["summary"], "task X done");
        assert_eq!(ev["priority"], "high");
        assert_eq!(ev["issue_id"], "ATN-42");
        assert_eq!(ev["source_repo"], ".");
        // Auto-generated id: `cli-<from>-<millis>`.
        assert!(ev["id"].as_str().unwrap().starts_with("cli-worker-hlasm-"));
        // RFC3339 timestamps start with year + `T` separator.
        let ts = ev["timestamp"].as_str().unwrap();
        assert!(ts.len() >= 20 && ts.contains('T'));
    }

    #[test]
    fn events_table_renders_broadcast_row() {
        let entries = vec![
            EventLogEntryLite {
                event: PushEventLite {
                    kind: "completion_notice".into(),
                    source_agent: "worker-hlasm".into(),
                    target_agent: Some("coordinator".into()),
                    summary: "task X done".into(),
                },
                decision: "deliver:coordinator".into(),
                delivered: true,
                logged_at: "2026-04-23T17:00:00Z".into(),
            },
            EventLogEntryLite {
                event: PushEventLite {
                    kind: "blocked_notice".into(),
                    source_agent: "worker-rpg".into(),
                    target_agent: None,
                    summary: "blocked on hlasm".into(),
                },
                decision: "broadcast".into(),
                delivered: false,
                logged_at: "2026-04-23T17:05:00Z".into(),
            },
        ];
        let out = format_events_table(&entries);
        let lines: Vec<&str> = out.lines().collect();
        // header + rule + 2 rows
        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("LOGGED_AT"));
        assert!(lines[2].contains("worker-hlasm → coordinator"));
        assert!(lines[2].contains("yes"));
        assert!(lines[3].contains("worker-rpg → broadcast"));
        assert!(lines[3].contains("no"));
    }

    #[test]
    fn screenshot_format_serializes_to_endpoint_strings() {
        assert_eq!(ScreenshotFormat::Text.as_str(), "text");
        assert_eq!(ScreenshotFormat::Ansi.as_str(), "ansi");
        assert_eq!(ScreenshotFormat::Html.as_str(), "html");
    }

    #[test]
    fn state_label_handles_simple_and_complex_agent_state() {
        assert_eq!(state_label(&json!({"state": "running"})), "running");
        // Blocked carries an `on` list; we still surface the short key.
        assert_eq!(
            state_label(&json!({"state": "blocked", "on": ["alice"]})),
            "blocked"
        );
        // Degenerate case: no `state` field → full JSON fallback.
        assert_eq!(state_label(&json!({"foo": 1})), "{\"foo\":1}");
    }
}
