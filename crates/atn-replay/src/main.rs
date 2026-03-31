use std::fs;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "atn-replay", about = "Display and replay ATN PTY transcripts")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show the final terminal state after replaying all bytes
    Screenshot {
        /// Path to transcript.log file
        path: PathBuf,

        /// Terminal rows (default: 40, matches ATN PTY)
        #[arg(long, default_value_t = 40)]
        rows: u16,

        /// Terminal columns (default: 120, matches ATN PTY)
        #[arg(long, default_value_t = 120)]
        cols: u16,

        /// Trim trailing empty lines
        #[arg(long, default_value_t = true)]
        trim: bool,

        /// Output as self-contained HTML page (for browser / Playwright)
        #[arg(long)]
        html: Option<PathBuf>,

        /// Output as embeddable HTML fragment (for org export — no body styles)
        #[arg(long)]
        html_fragment: Option<PathBuf>,

        /// Title shown in the header
        #[arg(long, default_value = "PTY Screenshot")]
        title: String,
    },

    /// Show terminal state at a specific byte offset
    At {
        /// Path to transcript.log file
        path: PathBuf,

        /// Byte offset to stop at
        offset: usize,

        /// Terminal rows
        #[arg(long, default_value_t = 40)]
        rows: u16,

        /// Terminal columns
        #[arg(long, default_value_t = 120)]
        cols: u16,

        /// Output as self-contained HTML page
        #[arg(long)]
        html: Option<PathBuf>,

        /// Output as embeddable HTML fragment
        #[arg(long)]
        html_fragment: Option<PathBuf>,

        /// Title shown in the header
        #[arg(long, default_value = "PTY Screenshot")]
        title: String,
    },

    /// Show terminal state at each chunk boundary (step-through mode)
    Steps {
        /// Path to transcript.log file
        path: PathBuf,

        /// Chunk size in bytes for stepping
        #[arg(long, default_value_t = 256)]
        chunk: usize,

        /// Terminal rows
        #[arg(long, default_value_t = 40)]
        rows: u16,

        /// Terminal columns
        #[arg(long, default_value_t = 120)]
        cols: u16,
    },

    /// Show all transcript files in an ATN log directory
    List {
        /// Path to .atn/logs directory (or auto-detect from cwd)
        #[arg(default_value = ".atn/logs")]
        path: PathBuf,
    },

    /// Print raw transcript with escape sequences stripped (plain text)
    Text {
        /// Path to transcript.log file
        path: PathBuf,
    },

    /// Generate an org-mode dashboard from all agent transcripts.
    ///
    /// Shows each agent's current terminal state as a text block, plus
    /// recent inputs. Designed for `auto-revert-mode` in emacs — re-run
    /// to refresh.
    Dashboard {
        /// Path to .atn/logs directory
        #[arg(default_value = ".atn/logs")]
        logs: PathBuf,

        /// Write output to file (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Terminal rows per agent
        #[arg(long, default_value_t = 40)]
        rows: u16,

        /// Terminal columns per agent
        #[arg(long, default_value_t = 120)]
        cols: u16,

        /// Number of visible terminal lines per agent (0 = all)
        #[arg(long, default_value_t = 25)]
        tail: usize,

        /// Number of recent input events to show per agent
        #[arg(long, default_value_t = 5)]
        recent_inputs: usize,
    },
}

// ── Terminal replay ─────────────────────────────────────────────────

fn replay_to_screen(data: &[u8], rows: u16, cols: u16) -> vt100::Screen {
    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(data);
    parser.screen().clone()
}

fn screen_to_lines(screen: &vt100::Screen, trim: bool) -> Vec<String> {
    let rows = screen.size().0;
    let cols = screen.size().1;

    let mut lines: Vec<String> = Vec::new();
    for row in 0..rows {
        let mut line = String::new();
        for col in 0..cols {
            let cell = screen.cell(row, col);
            match cell {
                Some(cell) => line.push(cell.contents().chars().next().unwrap_or(' ')),
                None => line.push(' '),
            }
        }
        lines.push(line.trim_end().to_string());
    }

    if trim {
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
    }

    lines
}

// ── Text output ─────────────────────────────────────────────────────

fn print_screen(screen: &vt100::Screen, trim: bool) {
    let lines = screen_to_lines(screen, trim);
    let cols = screen.size().1;
    let width = lines
        .iter()
        .map(|l| l.len())
        .max()
        .unwrap_or(0)
        .max(cols as usize);
    let border = "─".repeat(width + 2);
    println!("┌{border}┐");
    for line in &lines {
        println!("│ {line:<width$} │");
    }
    println!("└{border}┘");
}

// ── HTML output ─────────────────────────────────────────────────────

const TERMINAL_CSS: &str = r#"
  .atn-terminal {
    background: #0d1117;
    color: #c9d1d9;
    font-family: 'SF Mono', 'Menlo', 'Monaco', 'Courier New', monospace;
    font-size: 12px;
    line-height: 1.4;
    padding: 12px 16px;
    border-radius: 8px;
    border: 1px solid #30363d;
    box-shadow: 0 4px 12px rgba(0,0,0,0.4);
    white-space: pre;
    display: inline-block;
    min-width: 600px;
    overflow-x: auto;
  }
  .atn-terminal-header {
    display: flex; align-items: center;
    margin-bottom: 10px; padding-bottom: 8px;
    border-bottom: 1px solid #21262d;
  }
  .atn-dot { width:12px; height:12px; border-radius:50%; display:inline-block; margin-right:6px; }
  .atn-dot-red { background:#ff5f56; }
  .atn-dot-yellow { background:#ffbd2e; }
  .atn-dot-green { background:#27c93f; }
  .atn-terminal-title { color: #8b949e; font-size: 11px; margin-left: 12px; }
"#;

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn terminal_div(title: &str, body: &str) -> String {
    let title_escaped = escape_html(title);
    format!(
        r#"<div class="atn-terminal">
  <div class="atn-terminal-header">
    <span class="atn-dot atn-dot-red"></span>
    <span class="atn-dot atn-dot-yellow"></span>
    <span class="atn-dot atn-dot-green"></span>
    <span class="atn-terminal-title">{title_escaped}</span>
  </div>
{body}
</div>"#
    )
}

fn screen_to_html(screen: &vt100::Screen, title: &str, trim: bool) -> String {
    let lines = screen_to_lines(screen, trim);
    let body: String = lines
        .iter()
        .map(|l| escape_html(l))
        .collect::<Vec<_>>()
        .join("\n");
    let title_escaped = escape_html(title);

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>{title_escaped}</title>
<style>
  body {{ margin: 0; padding: 16px; background: #1a1a2e; display: flex; justify-content: center; }}
  {TERMINAL_CSS}
</style>
</head>
<body>
{div}
</body>
</html>"#,
        div = terminal_div(title, &body)
    )
}

fn screen_to_html_fragment(screen: &vt100::Screen, title: &str, trim: bool) -> String {
    let lines = screen_to_lines(screen, trim);
    let body: String = lines
        .iter()
        .map(|l| escape_html(l))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "<style>\n{TERMINAL_CSS}</style>\n{}",
        terminal_div(title, &body)
    )
}

// ── Org-mode dashboard ──────────────────────────────────────────────

fn read_recent_inputs(inputs_path: &std::path::Path, count: usize) -> Vec<String> {
    let Ok(content) = fs::read_to_string(inputs_path) else {
        return Vec::new();
    };
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(count);
    lines[start..]
        .iter()
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            let ts = v.get("ts")?.as_str()?;
            // Truncate to HH:MM:SS
            let short_ts = ts.get(11..19).unwrap_or(ts);
            let evt = v.get("event")?;
            let etype = evt.get("type")?.as_str()?;
            let detail = match etype {
                "human_text" => evt.get("text")?.as_str()?.to_string(),
                "coordinator_command" => {
                    format!("[coord] {}", evt.get("command")?.as_str()?)
                }
                "raw_bytes" => "[raw bytes]".to_string(),
                "action" => format!("[action] {:?}", evt.get("action")),
                _ => format!("[{etype}]"),
            };
            // Truncate long commands
            let detail = if detail.len() > 100 {
                format!("{}...", &detail[..97])
            } else {
                detail
            };
            Some(format!("{short_ts}  {detail}"))
        })
        .collect()
}

fn generate_dashboard(
    logs_dir: &std::path::Path,
    rows: u16,
    cols: u16,
    tail: usize,
    recent_inputs: usize,
) -> String {
    let mut out = String::new();

    // Header
    out.push_str("#+TITLE: ATN Agent Dashboard\n");
    out.push_str(&format!(
        "#+DATE: {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    ));
    out.push_str("#+STARTUP: showall\n");
    out.push_str("# Auto-generated by atn-replay dashboard. Use auto-revert-mode to refresh.\n\n");

    let mut agents: Vec<_> = fs::read_dir(logs_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .collect();
    agents.sort_by_key(|e| e.file_name());

    if agents.is_empty() {
        out.push_str("No agent transcripts found.\n");
        return out;
    }

    for entry in &agents {
        let agent_id = entry.file_name();
        let agent_id = agent_id.to_string_lossy();
        let agent_dir = entry.path();
        let transcript_path = agent_dir.join("transcript.log");
        let inputs_path = agent_dir.join("inputs.jsonl");

        let t_size = fs::metadata(&transcript_path).map(|m| m.len()).unwrap_or(0);

        out.push_str(&format!("* {agent_id}"));
        if t_size == 0 {
            out.push_str("  (no output yet)\n\n");
            continue;
        }
        out.push_str(&format!("  ({t_size} bytes)\n\n"));

        // Recent inputs
        if recent_inputs > 0 {
            let inputs = read_recent_inputs(&inputs_path, recent_inputs);
            if !inputs.is_empty() {
                out.push_str("** Recent inputs\n\n");
                out.push_str("#+begin_example\n");
                for input in &inputs {
                    out.push_str(input);
                    out.push('\n');
                }
                out.push_str("#+end_example\n\n");
            }
        }

        // Terminal state
        out.push_str("** Terminal\n\n");
        let data = fs::read(&transcript_path).unwrap_or_default();
        let screen = replay_to_screen(&data, rows, cols);
        let lines = screen_to_lines(&screen, true);

        // Show tail N lines (or all if tail == 0)
        let visible = if tail > 0 && lines.len() > tail {
            let skip = lines.len() - tail;
            out.push_str(&format!(
                "(showing last {tail} of {} lines)\n\n",
                lines.len()
            ));
            &lines[skip..]
        } else {
            &lines[..]
        };

        out.push_str("#+begin_example\n");
        for line in visible {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("#+end_example\n\n");
    }

    out
}

// ── Utilities ───────────────────────────────────────────────────────

fn load_transcript(path: &PathBuf) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|e| {
        eprintln!("Error reading {}: {e}", path.display());
        std::process::exit(1);
    })
}

fn write_file(path: &PathBuf, content: &str) {
    fs::write(path, content).unwrap_or_else(|e| {
        eprintln!("Error writing {}: {e}", path.display());
        std::process::exit(1);
    });
    eprintln!("Wrote {}", path.display());
}

// ── Main ────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Screenshot {
            path,
            rows,
            cols,
            trim,
            html,
            html_fragment,
            title,
        } => {
            let data = load_transcript(&path);
            let screen = replay_to_screen(&data, rows, cols);

            if let Some(out) = html.as_ref().or(html_fragment.as_ref()) {
                let content = if html.is_some() {
                    screen_to_html(&screen, &title, trim)
                } else {
                    screen_to_html_fragment(&screen, &title, trim)
                };
                write_file(out, &content);
            } else {
                println!("Transcript: {} ({} bytes)", path.display(), data.len());
                print_screen(&screen, trim);
            }
        }

        Command::At {
            path,
            offset,
            rows,
            cols,
            html,
            html_fragment,
            title,
        } => {
            let data = load_transcript(&path);
            let end = offset.min(data.len());
            let screen = replay_to_screen(&data[..end], rows, cols);

            if let Some(out) = html.as_ref().or(html_fragment.as_ref()) {
                let content = if html.is_some() {
                    screen_to_html(&screen, &title, true)
                } else {
                    screen_to_html_fragment(&screen, &title, true)
                };
                write_file(out, &content);
            } else {
                println!(
                    "Transcript: {} (showing first {} of {} bytes)",
                    path.display(),
                    end,
                    data.len()
                );
                print_screen(&screen, true);
            }
        }

        Command::Steps {
            path,
            chunk,
            rows,
            cols,
        } => {
            let data = load_transcript(&path);
            let mut offset = 0;
            let mut step = 0;
            while offset < data.len() {
                let end = (offset + chunk).min(data.len());
                step += 1;
                println!(
                    "\n=== Step {step} (bytes {offset}..{end} of {}) ===",
                    data.len()
                );
                let screen = replay_to_screen(&data[..end], rows, cols);
                print_screen(&screen, true);
                offset = end;
            }
        }

        Command::List { path } => {
            let entries = fs::read_dir(&path).unwrap_or_else(|e| {
                eprintln!("Error reading {}: {e}", path.display());
                std::process::exit(1);
            });

            println!("Agent transcripts in {}:", path.display());
            println!();
            let mut found = false;
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    let agent_id = entry.file_name();
                    let transcript = entry.path().join("transcript.log");
                    let events = entry.path().join("events.jsonl");
                    let inputs = entry.path().join("inputs.jsonl");

                    let t_size = fs::metadata(&transcript).map(|m| m.len()).unwrap_or(0);
                    let has_events = events.exists();
                    let has_inputs = inputs.exists();

                    if t_size > 0 {
                        found = true;
                        let mut extras = Vec::new();
                        if has_events {
                            extras.push("events.jsonl");
                        }
                        if has_inputs {
                            extras.push("inputs.jsonl");
                        }
                        let extra_str = if extras.is_empty() {
                            String::new()
                        } else {
                            format!("  + {}", extras.join(", "))
                        };
                        println!(
                            "  {:20} transcript.log: {:>6} bytes{extra_str}",
                            agent_id.to_string_lossy(),
                            t_size,
                        );
                    }
                }
            }
            if !found {
                println!("  (no transcripts found)");
            }
        }

        Command::Text { path } => {
            let data = load_transcript(&path);
            let screen = replay_to_screen(&data, 200, 200);
            print!("{}", screen.contents());
        }

        Command::Dashboard {
            logs,
            output,
            rows,
            cols,
            tail,
            recent_inputs,
        } => {
            let content = generate_dashboard(&logs, rows, cols, tail, recent_inputs);
            if let Some(out) = output {
                write_file(&out, &content);
            } else {
                print!("{content}");
            }
        }
    }
}
