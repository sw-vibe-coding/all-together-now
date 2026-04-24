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
    };
    ExitCode::from(code)
}

fn run_agents(base: &str, verbose: bool, cmd: AgentsCommand) -> u8 {
    match cmd {
        AgentsCommand::List(ListArgs { fmt }) => agents_list(base, verbose, fmt.format),
        AgentsCommand::State { id, fmt } => agents_state(base, verbose, &id, fmt.format),
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
