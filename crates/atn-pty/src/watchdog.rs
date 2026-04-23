//! Per-agent runtime watchdog state — a rolling `last_output_at`
//! timestamp plus `stalled` flag. Paired with
//! [`atn_core::watchdog::WatchdogConfig`] which carries the static
//! thresholds.
//!
//! The state tracker (see `state_tracker::spawn_state_tracker`) updates
//! this struct as PTY bytes / state transitions stream past. Step 6's
//! action layer and the `GET /api/agents/{id}/state` handler read it.

use std::time::Instant;

use atn_core::agent::AgentState;
use atn_core::watchdog::WatchdogConfig;

/// Runtime companion to `WatchdogConfig`.
#[derive(Debug, Clone)]
pub struct WatchdogState {
    pub config: WatchdogConfig,
    /// Timestamp of the most recent PTY output burst. `None` until
    /// bytes have been seen.
    pub last_output_at: Option<Instant>,
    /// Wall-clock instant the agent most recently transitioned INTO
    /// `Running`. Used by `max_running_secs`.
    pub running_since: Option<Instant>,
    /// True when stall-detection has fired for the current quiet
    /// period. Cleared as soon as any output resumes.
    pub stalled: bool,
    /// Instant we first flagged `stalled = true` for the current quiet
    /// period. Used to derive `stalled_for_secs` in the state endpoint.
    pub stalled_since: Option<Instant>,
}

impl WatchdogState {
    pub fn new(config: WatchdogConfig) -> Self {
        Self {
            config,
            last_output_at: None,
            running_since: None,
            stalled: false,
            stalled_since: None,
        }
    }

    /// Record a fresh PTY output burst. Resets the silence clock and
    /// clears any prior `stalled` flag.
    pub fn on_output(&mut self, now: Instant) {
        self.last_output_at = Some(now);
        if self.stalled {
            self.stalled = false;
            self.stalled_since = None;
        }
    }

    /// Reset the watchdog when the agent transitions out of `Running`
    /// to a state where silence is expected (Idle, AwaitingHumanInput,
    /// CompletedTask, Disconnected, ...).
    pub fn on_leaving_running(&mut self) {
        self.running_since = None;
        self.stalled = false;
        self.stalled_since = None;
    }

    /// Note the agent transitioned INTO `Running` at `now`. Only the
    /// first transition per quiet-running-period records the
    /// `running_since` — repeat notifications while already running
    /// leave it alone.
    pub fn on_entering_running(&mut self, now: Instant) {
        if self.running_since.is_none() {
            self.running_since = Some(now);
        }
    }

    /// Check the stall condition against the current state + clock.
    /// Returns `true` if `stalled` was flipped from `false` to `true`
    /// on this call — callers can use that to emit a one-shot event
    /// per stall event.
    pub fn check_stall(&mut self, now: Instant, state: &AgentState) -> bool {
        if !matches!(state, AgentState::Running) {
            return false;
        }
        let Some(last) = self.last_output_at else {
            return false;
        };
        let elapsed = now.saturating_duration_since(last);
        if elapsed.as_secs() >= self.config.stall_secs && !self.stalled {
            self.stalled = true;
            self.stalled_since = Some(now);
            return true;
        }
        false
    }

    /// Seconds since we first flagged the current stall (or `None` if
    /// not currently stalled).
    pub fn stalled_for_secs(&self, now: Instant) -> Option<u64> {
        self.stalled_since
            .map(|t| now.saturating_duration_since(t).as_secs())
    }

    /// Seconds the agent has been continuously in `Running` (or
    /// `None` if not currently running). Used by step 6's
    /// `max_running_secs` escalation.
    pub fn running_for_secs(&self, now: Instant) -> Option<u64> {
        self.running_since
            .map(|t| now.saturating_duration_since(t).as_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn base_config(stall_secs: u64) -> WatchdogConfig {
        WatchdogConfig {
            stall_secs,
            max_running_secs: None,
        }
    }

    #[test]
    fn fresh_watchdog_has_no_output_yet() {
        let w = WatchdogState::new(base_config(5));
        assert!(w.last_output_at.is_none());
        assert!(!w.stalled);
    }

    #[test]
    fn check_stall_noop_without_output() {
        let mut w = WatchdogState::new(base_config(5));
        let now = Instant::now();
        assert!(!w.check_stall(now, &AgentState::Running));
        assert!(!w.stalled);
    }

    #[test]
    fn check_stall_fires_after_threshold() {
        let mut w = WatchdogState::new(base_config(5));
        let t0 = Instant::now();
        w.on_output(t0);
        // Before threshold: no stall.
        assert!(!w.check_stall(t0 + Duration::from_secs(4), &AgentState::Running));
        assert!(!w.stalled);
        // After threshold: stall flips on, returns true ONCE.
        let fired = w.check_stall(t0 + Duration::from_secs(6), &AgentState::Running);
        assert!(fired);
        assert!(w.stalled);
        // Same stall condition on a later tick: already stalled, returns false.
        let fired_again = w.check_stall(t0 + Duration::from_secs(10), &AgentState::Running);
        assert!(!fired_again);
        assert!(w.stalled);
    }

    #[test]
    fn output_resumption_clears_stall() {
        let mut w = WatchdogState::new(base_config(5));
        let t0 = Instant::now();
        w.on_output(t0);
        assert!(w.check_stall(t0 + Duration::from_secs(6), &AgentState::Running));
        // Fresh output arrives; stall clears.
        w.on_output(t0 + Duration::from_secs(7));
        assert!(!w.stalled);
        // And a subsequent stall can fire again after another window of silence.
        let fired = w.check_stall(t0 + Duration::from_secs(13), &AgentState::Running);
        assert!(fired);
    }

    #[test]
    fn only_running_state_can_stall() {
        let mut w = WatchdogState::new(base_config(5));
        let t0 = Instant::now();
        w.on_output(t0);
        // Way past the threshold, but state is idle — no stall.
        assert!(!w.check_stall(t0 + Duration::from_secs(100), &AgentState::Idle));
        assert!(!w.check_stall(
            t0 + Duration::from_secs(100),
            &AgentState::AwaitingHumanInput
        ));
        assert!(!w.stalled);
    }

    #[test]
    fn stalled_for_secs_tracks_elapsed_from_first_stall() {
        let mut w = WatchdogState::new(base_config(5));
        let t0 = Instant::now();
        w.on_output(t0);
        w.check_stall(t0 + Duration::from_secs(6), &AgentState::Running);
        let e = w.stalled_for_secs(t0 + Duration::from_secs(10));
        assert_eq!(e, Some(4));
    }

    #[test]
    fn running_since_set_once_per_run() {
        let mut w = WatchdogState::new(base_config(5));
        let t0 = Instant::now();
        w.on_entering_running(t0);
        w.on_entering_running(t0 + Duration::from_secs(3)); // no-op
        assert_eq!(w.running_since, Some(t0));
        // Elapsed counts from the first transition.
        let e = w.running_for_secs(t0 + Duration::from_secs(10));
        assert_eq!(e, Some(10));
        // Leaving running clears it.
        w.on_leaving_running();
        assert!(w.running_since.is_none());
    }

    #[test]
    fn bursts_plus_quiet_gaps_emit_one_stall_per_event() {
        // Simulates: noisy for a bit, quiet, resumes, quiet again.
        let mut w = WatchdogState::new(base_config(5));
        let t0 = Instant::now();
        // Burst 1.
        for delta in 0..3 {
            w.on_output(t0 + Duration::from_secs(delta));
        }
        // Quiet period — stall should fire exactly once.
        let mut fires = 0;
        for delta in 4..12 {
            if w.check_stall(t0 + Duration::from_secs(delta), &AgentState::Running) {
                fires += 1;
            }
        }
        assert_eq!(fires, 1);
        // Burst 2.
        w.on_output(t0 + Duration::from_secs(13));
        assert!(!w.stalled);
        // Quiet period 2 — second stall event.
        let mut fires2 = 0;
        for delta in 14..22 {
            if w.check_stall(t0 + Duration::from_secs(delta), &AgentState::Running) {
                fires2 += 1;
            }
        }
        assert_eq!(fires2, 1);
    }
}
