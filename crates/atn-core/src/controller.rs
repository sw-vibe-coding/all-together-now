use crate::agent::{AgentConfig, AgentId, AgentState, AgentStatus};
use crate::error::Result;
use crate::event::InputEvent;

/// Public API trait for the Program Manager controller.
///
/// This is the library boundary for CLI/TUI/Emacs/Web frontends.
/// The concrete implementation lives in atn-server, which has access
/// to SessionManager (atn-pty), wiki storage (atn-wiki), and trail
/// reader (atn-trail).
pub trait PgmController {
    /// Spawn a new agent session.
    fn spawn_agent(
        &mut self,
        config: AgentConfig,
    ) -> impl std::future::Future<Output = Result<AgentId>> + Send;

    /// Send input to an agent's PTY.
    fn send_input(
        &self,
        id: &AgentId,
        event: InputEvent,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Get the current state of an agent.
    fn agent_state(
        &self,
        id: &AgentId,
    ) -> impl std::future::Future<Output = Result<AgentState>> + Send;

    /// Get the full status snapshot of an agent.
    fn agent_status(
        &self,
        id: &AgentId,
    ) -> impl std::future::Future<Output = Result<AgentStatus>> + Send;

    /// List all active agent IDs.
    fn list_agents(&self) -> Vec<AgentId>;

    /// Shut down a specific agent.
    fn shutdown_agent(
        &mut self,
        id: &AgentId,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Shut down all agents.
    fn shutdown_all(&mut self) -> impl std::future::Future<Output = Result<()>> + Send;
}
