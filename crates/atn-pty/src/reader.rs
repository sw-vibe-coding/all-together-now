use std::io::Read;

use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use atn_core::event::OutputSignal;

const READ_BUF_SIZE: usize = 8192;

/// Spawn a blocking task that reads from the PTY master and sends output signals.
pub fn spawn_reader_task(
    mut reader: Box<dyn Read + Send>,
    tx: broadcast::Sender<OutputSignal>,
) -> JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        let mut buf = [0u8; READ_BUF_SIZE];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = buf[..n].to_vec();
                    // Best-effort send — if no receivers, that's fine.
                    let _ = tx.send(OutputSignal::Bytes(chunk));
                }
                Err(e) => {
                    tracing::debug!("PTY read error (session likely closed): {e}");
                    break;
                }
            }
        }
    })
}
