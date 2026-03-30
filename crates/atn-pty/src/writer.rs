use std::io::Write;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use atn_core::event::{CannedAction, InputEvent};

/// Spawn a blocking task that consumes input events and writes to the PTY master.
///
/// All writes are serialized through the mpsc channel — no interleaving.
pub fn spawn_writer_task(
    mut writer: Box<dyn Write + Send>,
    mut rx: mpsc::Receiver<InputEvent>,
) -> JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        while let Some(event) = rx.blocking_recv() {
            let bytes = input_event_to_bytes(&event);
            if writer.write_all(&bytes).is_err() {
                break;
            }
            let _ = writer.flush();
        }
    })
}

fn input_event_to_bytes(event: &InputEvent) -> Vec<u8> {
    match event {
        InputEvent::HumanText { text } => format!("{text}\n").into_bytes(),
        InputEvent::RawBytes { bytes } => bytes.clone(),
        InputEvent::CoordinatorCommand { command } => format!("{command}\n").into_bytes(),
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
