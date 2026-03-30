use std::path::PathBuf;
use std::sync::Arc;

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tokio::sync::{broadcast, mpsc, RwLock};

use atn_core::agent::{AgentConfig, AgentId, AgentState};
use atn_core::error::{AtnError, Result};
use atn_core::event::{InputEvent, OutputSignal};

use crate::reader::spawn_reader_task;
use crate::transcript::TranscriptWriter;
use crate::writer::spawn_writer_task;

const PTY_ROWS: u16 = 40;
const PTY_COLS: u16 = 120;
const OUTPUT_CHANNEL_CAPACITY: usize = 256;
const INPUT_CHANNEL_CAPACITY: usize = 64;

/// A managed PTY session for one agent.
pub struct PtySession {
    agent_id: AgentId,
    child: Box<dyn portable_pty::Child + Send>,
    input_tx: mpsc::Sender<InputEvent>,
    output_tx: broadcast::Sender<OutputSignal>,
    state: Arc<RwLock<AgentState>>,
    _master: Box<dyn MasterPty + Send>,
    _reader_handle: tokio::task::JoinHandle<()>,
    _writer_handle: tokio::task::JoinHandle<()>,
    _transcript_handle: Option<tokio::task::JoinHandle<()>>,
}

impl PtySession {
    /// Spawn a new PTY session for the given agent configuration.
    ///
    /// If `log_dir` is provided, transcript and event logs are written to
    /// `{log_dir}/{agent_id}/`.
    pub async fn spawn(config: &AgentConfig, log_dir: Option<PathBuf>) -> Result<Self> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize {
                rows: PTY_ROWS,
                cols: PTY_COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| AtnError::Pty(e.to_string()))?;

        let mut cmd = CommandBuilder::new("bash");
        cmd.cwd(&config.repo_path);

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| AtnError::Pty(e.to_string()))?;

        // Drop slave — we only interact via the master side.
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| AtnError::Pty(e.to_string()))?;

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| AtnError::Pty(e.to_string()))?;

        let (output_tx, _) = broadcast::channel(OUTPUT_CHANNEL_CAPACITY);
        let (input_tx, input_rx) = mpsc::channel(INPUT_CHANNEL_CAPACITY);
        let state = Arc::new(RwLock::new(AgentState::Starting));

        let reader_handle = spawn_reader_task(reader, output_tx.clone());
        let writer_handle = spawn_writer_task(writer, input_rx);

        // Optionally start transcript logging.
        let transcript_handle = if let Some(dir) = log_dir {
            let agent_dir = dir.join(&config.id.0);
            let tw = TranscriptWriter::new(&agent_dir)?;
            let rx = output_tx.subscribe();
            Some(tw.spawn(rx))
        } else {
            None
        };

        // Inject setup commands.
        let setup_tx = input_tx.clone();
        let setup_commands = config.setup_commands.clone();
        let launch = config.launch_command.clone();
        let state_clone = state.clone();
        tokio::spawn(async move {
            // Set a known prompt for the outer shell.
            let _ = setup_tx
                .send(InputEvent::CoordinatorCommand {
                    command: r#"export PS1="__ATN_READY__> ""#.to_string(),
                })
                .await;

            for cmd in &setup_commands {
                let _ = setup_tx
                    .send(InputEvent::CoordinatorCommand {
                        command: cmd.clone(),
                    })
                    .await;
            }

            if !launch.is_empty() {
                let _ = setup_tx
                    .send(InputEvent::CoordinatorCommand {
                        command: launch,
                    })
                    .await;
            }

            let mut s = state_clone.write().await;
            *s = AgentState::Running;
        });

        Ok(Self {
            agent_id: config.id.clone(),
            child,
            input_tx,
            output_tx,
            state,
            _master: pair.master,
            _reader_handle: reader_handle,
            _writer_handle: writer_handle,
            _transcript_handle: transcript_handle,
        })
    }

    /// The agent ID for this session.
    pub fn agent_id(&self) -> &AgentId {
        &self.agent_id
    }

    /// Send an input event to the agent's PTY.
    pub async fn send_input(&self, event: InputEvent) -> Result<()> {
        self.input_tx
            .send(event)
            .await
            .map_err(|e| AtnError::Channel(e.to_string()))
    }

    /// Send Ctrl-C (0x03) to the agent's PTY.
    pub async fn send_ctrl_c(&self) -> Result<()> {
        self.send_input(InputEvent::RawBytes {
            bytes: vec![0x03],
        })
        .await
    }

    /// Get a new receiver for the agent's output stream.
    pub fn output_receiver(&self) -> broadcast::Receiver<OutputSignal> {
        self.output_tx.subscribe()
    }

    /// Get a clone of the agent's state handle.
    pub fn state(&self) -> Arc<RwLock<AgentState>> {
        self.state.clone()
    }

    /// Shut down the agent session.
    ///
    /// Sends Ctrl-C twice (1s apart), waits for exit, then kills if needed.
    pub async fn shutdown(&mut self) -> Result<()> {
        // First Ctrl-C
        let _ = self.send_ctrl_c().await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // Second Ctrl-C
        let _ = self.send_ctrl_c().await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Try to kill the child process.
        self.child
            .kill()
            .map_err(|e| AtnError::Pty(format!("kill failed: {e}")))?;

        let mut s = self.state.write().await;
        *s = AgentState::Disconnected;

        Ok(())
    }
}
