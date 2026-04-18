use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use atn_core::error::Result;
use atn_core::event::OutputSignal;

/// Writes raw PTY output to `transcript.log` and structured events to `events.jsonl`.
pub struct TranscriptWriter {
    transcript_path: PathBuf,
    events_path: PathBuf,
}

impl TranscriptWriter {
    /// Create a new transcript writer for the given agent directory.
    ///
    /// Creates the directory if it does not exist.
    pub fn new(agent_dir: &Path) -> Result<Self> {
        fs::create_dir_all(agent_dir)?;
        Ok(Self {
            transcript_path: agent_dir.join("transcript.log"),
            events_path: agent_dir.join("events.jsonl"),
        })
    }

    /// Spawn a blocking task that consumes output signals and writes to log files.
    pub fn spawn(self, mut rx: broadcast::Receiver<OutputSignal>) -> JoinHandle<()> {
        tokio::spawn(async move {
            while let Ok(signal) = rx.recv().await {
                // Best-effort writes — don't crash if logging fails.
                let _ = self.handle_signal(&signal);
            }
        })
    }

    fn handle_signal(&self, signal: &OutputSignal) -> Result<()> {
        match signal {
            OutputSignal::Bytes(data) => {
                let mut f = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.transcript_path)?;
                f.write_all(data)?;
            }
            _ => {
                self.write_event(signal)?;
            }
        }
        Ok(())
    }

    fn write_event(&self, signal: &OutputSignal) -> Result<()> {
        let event = match signal {
            OutputSignal::Bytes(_) => return Ok(()),
            OutputSignal::PromptReady => serde_json::json!({
                "type": "prompt_ready",
                "ts": chrono::Utc::now().to_rfc3339(),
            }),
            OutputSignal::QuestionDetected { snippet } => serde_json::json!({
                "type": "question_detected",
                "snippet": snippet,
                "ts": chrono::Utc::now().to_rfc3339(),
            }),
            OutputSignal::IdleDetected => serde_json::json!({
                "type": "idle_detected",
                "ts": chrono::Utc::now().to_rfc3339(),
            }),
            OutputSignal::PushEvent(pe) => serde_json::json!({
                "type": "push_event",
                "event": pe,
                "ts": chrono::Utc::now().to_rfc3339(),
            }),
            OutputSignal::Disconnected => serde_json::json!({
                "type": "disconnected",
                "ts": chrono::Utc::now().to_rfc3339(),
            }),
        };

        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.events_path)?;
        let line = serde_json::to_string(&event)?;
        writeln!(f, "{line}")?;
        Ok(())
    }
}

/// Open a transcript.log file and return its contents as bytes.
pub fn read_transcript(agent_dir: &Path) -> Result<Vec<u8>> {
    Ok(fs::read(agent_dir.join("transcript.log"))?)
}

/// Open an events.jsonl file and return all event lines.
pub fn read_events(agent_dir: &Path) -> Result<Vec<serde_json::Value>> {
    let content = fs::read_to_string(agent_dir.join("events.jsonl"))?;
    let mut events = Vec::new();
    for line in content.lines() {
        if !line.trim().is_empty() {
            events.push(serde_json::from_str(line)?);
        }
    }
    Ok(events)
}
