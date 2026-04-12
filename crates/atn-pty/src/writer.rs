use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use atn_core::event::{CannedAction, InputEvent};

/// Spawn a blocking task that consumes input events and writes to the PTY master.
///
/// All writes are serialized through the mpsc channel — no interleaving.
/// If `log_path` is provided, each input event is also logged to `inputs.jsonl`.
pub fn spawn_writer_task(
    mut writer: Box<dyn Write + Send>,
    mut rx: mpsc::Receiver<InputEvent>,
    log_path: Option<PathBuf>,
) -> JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        while let Some(event) = rx.blocking_recv() {
            // Log input event before writing to PTY.
            if let Some(ref path) = log_path {
                let _ = log_input_event(path, &event);
            }
            let bytes = input_event_to_bytes(&event);
            if writer.write_all(&bytes).is_err() {
                break;
            }
            let _ = writer.flush();
        }
    })
}

fn log_input_event(path: &PathBuf, event: &InputEvent) -> std::io::Result<()> {
    let entry = serde_json::json!({
        "event": event,
        "ts": chrono::Utc::now().to_rfc3339(),
    });
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}", serde_json::to_string(&entry).unwrap_or_default())
}

fn input_event_to_bytes(event: &InputEvent) -> Vec<u8> {
    match event {
        InputEvent::HumanText { text } => format!("{text}\r").into_bytes(),
        InputEvent::RawBytes { bytes } => bytes.clone(),
        InputEvent::CoordinatorCommand { command } => format!("{command}\r").into_bytes(),
        InputEvent::Action { action } => canned_action_to_bytes(action),
    }
}

fn canned_action_to_bytes(action: &CannedAction) -> Vec<u8> {
    match action {
        CannedAction::CtrlC => vec![0x03],
        CannedAction::ClaudeGo => b"claude go\n".to_vec(),
        CannedAction::ReadWiki { page } => format!("coord read {page}\n").into_bytes(),
        CannedAction::Ack { request_id } => format!("coord ack {request_id}\n").into_bytes(),
    }
}
