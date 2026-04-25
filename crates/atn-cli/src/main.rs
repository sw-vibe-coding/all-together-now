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
    /// Wiki pages — list, read, write, delete.
    Wiki {
        #[command(subcommand)]
        action: WikiCommand,
    },
    /// PR registry surfaced by atn-syncd — list, show, merge, reject.
    Prs {
        #[command(subcommand)]
        action: PrsCommand,
    },
}

#[derive(Subcommand)]
enum PrsCommand {
    /// List PR records (GET /api/prs).
    List {
        /// Filter by status.
        #[arg(long, value_enum)]
        status: Option<PrStatusArg>,
        #[command(flatten)]
        fmt: FormatArg,
    },
    /// Show a single PR record (GET /api/prs/{id}).
    Show {
        /// PR id (e.g. `alice-feature-7d80570`).
        id: String,
        #[command(flatten)]
        fmt: FormatArg,
    },
    /// Merge a PR into the central repo (POST /api/prs/{id}/merge).
    Merge {
        /// PR id.
        id: String,
    },
    /// Reject a PR (POST /api/prs/{id}/reject) — no git side-effects.
    Reject {
        /// PR id.
        id: String,
    },
}

#[derive(Copy, Clone, ValueEnum, Debug)]
enum PrStatusArg {
    Open,
    Merged,
    Rejected,
}

impl PrStatusArg {
    fn as_str(self) -> &'static str {
        match self {
            PrStatusArg::Open => "open",
            PrStatusArg::Merged => "merged",
            PrStatusArg::Rejected => "rejected",
        }
    }
}

#[derive(Subcommand)]
enum WikiCommand {
    /// List all wiki page titles (GET /api/wiki).
    List {
        #[command(flatten)]
        fmt: FormatArg,
    },
    /// Fetch a page's content. ETag printed to stderr with --verbose.
    Get {
        /// Page title (e.g. `Coordination/Goals`).
        title: String,
    },
    /// Create or update a page.
    Put {
        /// Page title.
        title: String,
        /// Read content from this file. Mutually exclusive with --stdin.
        #[arg(long, value_name = "PATH")]
        file: Option<String>,
        /// Read content from stdin. Mutually exclusive with --file.
        #[arg(long)]
        stdin: bool,
        /// Optimistic-concurrency ETag from a prior GET. Required when
        /// updating an existing page; omit only when creating fresh.
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
    },
    /// Delete a page.
    Delete {
        /// Page title.
        title: String,
        /// ETag from a prior GET. Required by the server.
        #[arg(long, value_name = "ETAG")]
        if_match: Option<String>,
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
        Command::Wiki { action } => run_wiki(&base, cli.verbose, action),
        Command::Prs { action } => run_prs(&base, cli.verbose, action),
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

// ── Wiki subcommands ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct WikiPageLite {
    #[serde(default)]
    content: String,
}

fn run_wiki(base: &str, verbose: bool, cmd: WikiCommand) -> u8 {
    match cmd {
        WikiCommand::List { fmt } => wiki_list(base, verbose, fmt.format),
        WikiCommand::Get { title } => wiki_get(base, verbose, &title),
        WikiCommand::Put {
            title,
            file,
            stdin,
            if_match,
        } => wiki_put(base, verbose, &title, file, stdin, if_match),
        WikiCommand::Delete { title, if_match } => {
            wiki_delete(base, verbose, &title, if_match)
        }
    }
}

fn wiki_list(base: &str, verbose: bool, format: OutputFormat) -> u8 {
    let url = format!("{base}/api/wiki");
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
    let titles: Vec<String> = match serde_json::from_str(&body) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "failed to parse wiki list: {e}");
            return EXIT_HTTP;
        }
    };
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&titles).unwrap_or(body)
            );
        }
        OutputFormat::Table => {
            if titles.is_empty() {
                println!("(no wiki pages)");
            } else {
                for t in titles {
                    println!("{t}");
                }
            }
        }
    }
    EXIT_OK
}

fn wiki_get(base: &str, verbose: bool, title: &str) -> u8 {
    // The server's {*title} route matches on a raw path segment — pass
    // through unencoded so titles like `Coordination/Goals` keep their
    // slash. (Encoding the slash would break the wildcard match.)
    let url = format!("{base}/api/wiki/{title}");
    if verbose {
        let _ = writeln!(std::io::stderr(), "GET {url}");
    }
    let resp = match ureq::get(&url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(404, _)) => {
            let _ = writeln!(std::io::stderr(), "wiki page '{title}' not found");
            return EXIT_NOT_FOUND;
        }
        Err(e) => return report_http_error(&url, e),
    };
    let status = resp.status();
    let etag = resp
        .header("ETag")
        .map(|s| s.to_string())
        .unwrap_or_default();
    let body = resp.into_string().unwrap_or_default();
    if !(200..300).contains(&status) {
        return report_status_error(&url, status, &body);
    }
    if verbose && !etag.is_empty() {
        let _ = writeln!(std::io::stderr(), "ETag: {etag}");
    }
    // The GET response is JSON; stdout gets the markdown body.
    let page: WikiPageLite = match serde_json::from_str(&body) {
        Ok(p) => p,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "failed to parse page: {e}");
            return EXIT_HTTP;
        }
    };
    print!("{}", page.content);
    EXIT_OK
}

fn read_body(file: Option<String>, stdin: bool) -> Result<String, String> {
    match (file, stdin) {
        (Some(_), true) => Err("--file and --stdin are mutually exclusive".to_string()),
        (Some(path), false) => std::fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {path}: {e}")),
        (None, true) => {
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
                .map(|_| buf)
                .map_err(|e| format!("failed to read stdin: {e}"))
        }
        (None, false) => Err("expected --file <path> or --stdin".to_string()),
    }
}

fn wiki_put(
    base: &str,
    verbose: bool,
    title: &str,
    file: Option<String>,
    stdin: bool,
    if_match: Option<String>,
) -> u8 {
    let content = match read_body(file, stdin) {
        Ok(c) => c,
        Err(msg) => {
            let _ = writeln!(std::io::stderr(), "{msg}");
            return EXIT_USAGE;
        }
    };
    let url = format!("{base}/api/wiki/{title}");
    if verbose {
        let _ = writeln!(
            std::io::stderr(),
            "PUT {url} ({} bytes){}",
            content.len(),
            if_match
                .as_deref()
                .map(|e| format!(" If-Match: {e}"))
                .unwrap_or_default()
        );
    }
    let mut req = ureq::put(&url).set("Content-Type", "application/json");
    if let Some(etag) = &if_match {
        req = req.set("If-Match", etag);
    }
    let body = serde_json::json!({ "content": content });
    match req.send_json(body) {
        Ok(resp) => {
            if let Some(new_etag) = resp.header("ETag")
                && verbose
            {
                let _ = writeln!(std::io::stderr(), "ETag: {new_etag}");
            }
            EXIT_OK
        }
        Err(ureq::Error::Status(409, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            report_etag_conflict(title, &body)
        }
        Err(ureq::Error::Status(404, _)) => {
            let _ = writeln!(std::io::stderr(), "wiki page '{title}' not found");
            EXIT_NOT_FOUND
        }
        Err(e) => report_http_error(&url, e),
    }
}

fn wiki_delete(
    base: &str,
    verbose: bool,
    title: &str,
    if_match: Option<String>,
) -> u8 {
    let url = format!("{base}/api/wiki/{title}");
    if verbose {
        let _ = writeln!(
            std::io::stderr(),
            "DELETE {url}{}",
            if_match
                .as_deref()
                .map(|e| format!(" If-Match: {e}"))
                .unwrap_or_default()
        );
    }
    let mut req = ureq::delete(&url);
    if let Some(etag) = &if_match {
        req = req.set("If-Match", etag);
    }
    match req.call() {
        Ok(_) => EXIT_OK,
        Err(ureq::Error::Status(409, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            report_etag_conflict(title, &body)
        }
        Err(ureq::Error::Status(404, _)) => {
            let _ = writeln!(std::io::stderr(), "wiki page '{title}' not found");
            EXIT_NOT_FOUND
        }
        Err(e) => report_http_error(&url, e),
    }
}

/// Pretty-print the server's `WikiConflictResponse` body (carries the
/// current ETag + the page the client was writing against). Exits 2 so
/// script loops can branch on "refetch + retry" cleanly.
fn report_etag_conflict(title: &str, body: &str) -> u8 {
    let etag = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("current_etag").and_then(|t| t.as_str()).map(String::from))
        .unwrap_or_default();
    if etag.is_empty() {
        let _ = writeln!(
            std::io::stderr(),
            "ETag mismatch for '{title}' — refetch and retry"
        );
    } else {
        let _ = writeln!(
            std::io::stderr(),
            "ETag mismatch for '{title}' — refetch and retry (current ETag: {etag})"
        );
    }
    EXIT_NOT_FOUND
}

// ── PR subcommands ───────────────────────────────────────────────────

fn run_prs(base: &str, verbose: bool, cmd: PrsCommand) -> u8 {
    match cmd {
        PrsCommand::List { status, fmt } => prs_list(base, verbose, status, fmt.format),
        PrsCommand::Show { id, fmt } => prs_show(base, verbose, &id, fmt.format),
        PrsCommand::Merge { id } => prs_action(base, verbose, &id, "merge"),
        PrsCommand::Reject { id } => prs_action(base, verbose, &id, "reject"),
    }
}

fn prs_list(
    base: &str,
    verbose: bool,
    status: Option<PrStatusArg>,
    format: OutputFormat,
) -> u8 {
    let url = match status {
        Some(s) => format!("{base}/api/prs?status={}", s.as_str()),
        None => format!("{base}/api/prs"),
    };
    if verbose {
        let _ = writeln!(std::io::stderr(), "GET {url}");
    }
    let resp = match ureq::get(&url).call() {
        Ok(r) => r,
        Err(e) => return report_http_error(&url, e),
    };
    let status_code = resp.status();
    let body = match resp.into_string() {
        Ok(s) => s,
        Err(e) => return report_transport_error(&url, e),
    };
    if !(200..300).contains(&status_code) {
        return report_status_error(&url, status_code, &body);
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
            let prs: Vec<atn_core::pr::PrRecord> = match serde_json::from_str(&body) {
                Ok(p) => p,
                Err(e) => {
                    let _ = writeln!(std::io::stderr(), "failed to parse prs: {e}");
                    return EXIT_HTTP;
                }
            };
            print_prs_table(&prs);
            EXIT_OK
        }
    }
}

fn prs_show(base: &str, verbose: bool, id: &str, format: OutputFormat) -> u8 {
    let url = format!("{base}/api/prs/{id}");
    if verbose {
        let _ = writeln!(std::io::stderr(), "GET {url}");
    }
    let resp = match ureq::get(&url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(404, _)) => {
            let _ = writeln!(std::io::stderr(), "pr '{id}' not found");
            return EXIT_NOT_FOUND;
        }
        Err(e) => return report_http_error(&url, e),
    };
    let status_code = resp.status();
    let body = resp.into_string().unwrap_or_default();
    if !(200..300).contains(&status_code) {
        return report_status_error(&url, status_code, &body);
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
            let pr: atn_core::pr::PrRecord = match serde_json::from_str(&body) {
                Ok(p) => p,
                Err(e) => {
                    let _ = writeln!(std::io::stderr(), "failed to parse pr: {e}");
                    return EXIT_HTTP;
                }
            };
            print!("{}", format_pr_show(&pr));
            EXIT_OK
        }
    }
}

fn prs_action(base: &str, verbose: bool, id: &str, action: &str) -> u8 {
    let url = format!("{base}/api/prs/{id}/{action}");
    if verbose {
        let _ = writeln!(std::io::stderr(), "POST {url}");
    }
    let resp = ureq::post(&url).send_string("");
    match resp {
        Ok(r) => {
            let status = r.status();
            let body = r.into_string().unwrap_or_default();
            if !(200..300).contains(&status) {
                return report_status_error(&url, status, &body);
            }
            // Echo the updated record so scripts can pipe it.
            match serde_json::from_str::<Value>(&body) {
                Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap_or(body)),
                Err(_) => println!("{body}"),
            }
            EXIT_OK
        }
        Err(ureq::Error::Status(404, _)) => {
            let _ = writeln!(std::io::stderr(), "pr '{id}' not found");
            EXIT_NOT_FOUND
        }
        Err(ureq::Error::Status(409, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            report_pr_conflict(action, id, &body)
        }
        Err(e) => report_http_error(&url, e),
    }
}

/// Pretty-print the server's 409 body — `{error, stderr}` for merge,
/// `{error, status}` for "not open". Surfaces the most useful field
/// to stderr and exits 2 (matches the wiki ETag mismatch convention).
fn report_pr_conflict(action: &str, id: &str, body: &str) -> u8 {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let error_msg = parsed
        .as_ref()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()))
        .unwrap_or("conflict");
    let stderr_msg = parsed
        .as_ref()
        .and_then(|v| v.get("stderr").and_then(|e| e.as_str()));
    let status_msg = parsed
        .as_ref()
        .and_then(|v| v.get("status").and_then(|e| e.as_str()));
    let _ = writeln!(
        std::io::stderr(),
        "atn-cli: {action} {id} failed: {error_msg}"
    );
    if let Some(s) = stderr_msg
        && !s.is_empty()
    {
        let _ = writeln!(std::io::stderr(), "{}", s.trim_end());
    } else if let Some(s) = status_msg {
        let _ = writeln!(std::io::stderr(), "current status: {s}");
    }
    EXIT_NOT_FOUND
}

fn print_prs_table(prs: &[atn_core::pr::PrRecord]) {
    if prs.is_empty() {
        println!("(no prs)");
        return;
    }
    print!("{}", format_prs_table(prs));
}

/// Build the table for `prs list`. Pure (no stdout) — caller prints.
fn format_prs_table(prs: &[atn_core::pr::PrRecord]) -> String {
    let headers = ["ID", "AGENT", "BRANCH → TARGET", "STATUS", "SUMMARY"];
    let rows: Vec<[String; 5]> = prs
        .iter()
        .map(|p| {
            [
                p.id.clone(),
                p.agent_id.clone(),
                format!("{} → {}", p.branch, p.target),
                pr_status_str(&p.status).to_string(),
                truncate(&p.summary, 80),
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

/// Build the `prs show` table — one `key: value` line per field.
fn format_pr_show(pr: &atn_core::pr::PrRecord) -> String {
    let mut out = String::new();
    out.push_str(&format!("id:           {}\n", pr.id));
    out.push_str(&format!("agent:        {}\n", pr.agent_id));
    out.push_str(&format!("branch:       {}\n", pr.branch));
    out.push_str(&format!("target:       {}\n", pr.target));
    out.push_str(&format!("source_repo:  {}\n", pr.source_repo));
    out.push_str(&format!("commit:       {}\n", pr.commit));
    out.push_str(&format!("status:       {}\n", pr_status_str(&pr.status)));
    out.push_str(&format!("created_at:   {}\n", pr.created_at));
    if let Some(c) = &pr.merge_commit {
        out.push_str(&format!("merge_commit: {c}\n"));
    }
    if let Some(t) = &pr.merged_at {
        out.push_str(&format!("merged_at:    {t}\n"));
    }
    if let Some(t) = &pr.rejected_at {
        out.push_str(&format!("rejected_at:  {t}\n"));
    }
    if let Some(e) = &pr.last_error {
        out.push_str(&format!("last_error:   {e}\n"));
    }
    out.push_str(&format!("summary:      {}\n", pr.summary));
    out
}

fn pr_status_str(s: &atn_core::pr::PrStatus) -> &'static str {
    match s {
        atn_core::pr::PrStatus::Open => "open",
        atn_core::pr::PrStatus::Merged => "merged",
        atn_core::pr::PrStatus::Rejected => "rejected",
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max - 1).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Serializes the three env-mutating base_url tests below.
    /// Cargo runs unit tests in parallel by default; without this
    /// guard concurrent ATN_URL writes flake at random.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    #[test]
    fn base_url_precedence_flag_wins() {
        let _guard = env_lock();
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
        let _guard = env_lock();
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
        let _guard = env_lock();
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
    fn read_body_rejects_mutually_exclusive_sources() {
        let err = read_body(Some("x".into()), true).unwrap_err();
        assert!(err.contains("mutually exclusive"));
    }

    #[test]
    fn read_body_requires_at_least_one_source() {
        let err = read_body(None, false).unwrap_err();
        assert!(err.contains("--file") && err.contains("--stdin"));
    }

    #[test]
    fn read_body_reads_from_file() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("atn-cli-body-{}.md", std::process::id()));
        std::fs::write(&path, "hello from file").unwrap();
        let got = read_body(Some(path.display().to_string()), false).unwrap();
        assert_eq!(got, "hello from file");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn etag_conflict_exits_not_found() {
        // Confirms the exit-code contract — scripts branch on `$? == 2`.
        let body = serde_json::to_string(&json!({
            "error": "ETag mismatch — page was modified",
            "current_etag": "etag-abc",
            "page": { "title": "T", "content": "x", "html": "x", "created_at": 1, "updated_at": 2 },
        }))
        .unwrap();
        assert_eq!(report_etag_conflict("T", &body), EXIT_NOT_FOUND);
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

    fn sample_pr() -> atn_core::pr::PrRecord {
        atn_core::pr::PrRecord {
            id: "alice-feature-7d80570".into(),
            agent_id: "alice".into(),
            source_repo: "/tmp/work".into(),
            branch: "feature".into(),
            target: "main".into(),
            commit: "7d8057045f89".into(),
            summary: "feature ready for review".into(),
            status: atn_core::pr::PrStatus::Open,
            created_at: "2026-04-25T00:00:00Z".into(),
            merge_commit: None,
            merged_at: None,
            rejected_at: None,
            last_error: None,
        }
    }

    #[test]
    fn prs_table_lays_out_columns_and_padding() {
        let prs = vec![sample_pr()];
        let out = format_prs_table(&prs);
        let lines: Vec<&str> = out.lines().collect();
        // Header + dashed rule + 1 row.
        assert_eq!(lines.len(), 3);
        for header in ["ID", "AGENT", "BRANCH → TARGET", "STATUS", "SUMMARY"] {
            assert!(
                lines[0].contains(header),
                "missing header {header} in {:?}",
                lines[0]
            );
        }
        assert!(lines[2].contains("alice-feature-7d80570"));
        assert!(lines[2].contains("feature → main"));
        assert!(lines[2].contains("open"));
        assert!(lines[2].contains("feature ready for review"));
    }

    #[test]
    fn prs_table_truncates_long_summary() {
        let mut pr = sample_pr();
        pr.summary = "A".repeat(200);
        let out = format_prs_table(&[pr]);
        let row = out.lines().nth(2).unwrap();
        // 80-char cap means the cell shows 79 As + the ellipsis,
        // which serializes as 3 UTF-8 bytes.
        assert!(row.contains("…"));
        let count_a = row.matches('A').count();
        assert_eq!(count_a, 79, "expected 79 As before the ellipsis, got {count_a}");
    }

    #[test]
    fn prs_table_empty_helper_prints_marker() {
        // The pure formatter still produces header + rule for empty input.
        let out = format_prs_table(&[]);
        assert_eq!(out.lines().count(), 2);
        assert!(out.contains("ID"));
    }

    #[test]
    fn pr_show_includes_optional_fields_when_set() {
        let mut pr = sample_pr();
        pr.status = atn_core::pr::PrStatus::Merged;
        pr.merge_commit = Some("aaa1111".into());
        pr.merged_at = Some("2026-04-25T01:00:00Z".into());
        pr.last_error = Some("flaked once but recovered".into());
        let out = format_pr_show(&pr);
        assert!(out.contains("status:       merged"));
        assert!(out.contains("merge_commit: aaa1111"));
        assert!(out.contains("merged_at:    2026-04-25T01:00:00Z"));
        assert!(out.contains("last_error:   flaked once but recovered"));
        assert!(!out.contains("rejected_at"), "rejected_at should be omitted when None");
    }

    #[test]
    fn pr_show_for_open_pr_omits_lifecycle_fields() {
        let pr = sample_pr();
        let out = format_pr_show(&pr);
        assert!(out.contains("status:       open"));
        for omitted in ["merge_commit", "merged_at", "rejected_at", "last_error"] {
            assert!(
                !out.contains(omitted),
                "expected open PR to omit {omitted}; got: {out}"
            );
        }
    }

    #[test]
    fn pr_status_str_round_trip() {
        assert_eq!(pr_status_str(&atn_core::pr::PrStatus::Open), "open");
        assert_eq!(pr_status_str(&atn_core::pr::PrStatus::Merged), "merged");
        assert_eq!(pr_status_str(&atn_core::pr::PrStatus::Rejected), "rejected");
    }

    #[test]
    fn pr_status_arg_url_encodes_filter() {
        assert_eq!(PrStatusArg::Open.as_str(), "open");
        assert_eq!(PrStatusArg::Merged.as_str(), "merged");
        assert_eq!(PrStatusArg::Rejected.as_str(), "rejected");
    }

    #[test]
    fn truncate_keeps_short_strings_intact() {
        assert_eq!(truncate("hello", 80), "hello");
        assert_eq!(truncate("", 80), "");
    }

    #[test]
    fn truncate_trims_with_ellipsis_at_max() {
        let s: String = "x".repeat(100);
        let out = truncate(&s, 80);
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().count(), 80);
    }

    #[test]
    fn pr_conflict_with_stderr_surfaces_to_user() {
        let body = serde_json::to_string(&json!({
            "error": "merge failed",
            "stderr": "CONFLICT (content): Merge conflict in a.txt\nAutomatic merge failed; fix conflicts and then commit the result.",
        }))
        .unwrap();
        assert_eq!(
            report_pr_conflict("merge", "alice-feature-abc1234", &body),
            EXIT_NOT_FOUND
        );
    }

    #[test]
    fn pr_conflict_with_status_field_falls_back_cleanly() {
        // The "PR not Open" branch on merge/reject sends `{error, status}`
        // (no stderr); make sure we don't choke.
        let body = serde_json::to_string(&json!({
            "error": "pr is not open",
            "status": "merged",
        }))
        .unwrap();
        assert_eq!(
            report_pr_conflict("merge", "alice-feature-abc1234", &body),
            EXIT_NOT_FOUND
        );
    }

    #[test]
    fn pr_conflict_with_garbage_body_still_exits_2() {
        // Defensive: a 409 with an unparseable body still exits 2 cleanly.
        assert_eq!(
            report_pr_conflict("merge", "alice-feature-abc1234", "not json"),
            EXIT_NOT_FOUND
        );
    }
}
