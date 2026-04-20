//! Per-agent "heat" tracking for the scale-UI layout.
//!
//! Every live agent carries a `HeatState` that blends:
//! - an EWMA of its output bytes-per-second (the data-driven signal), and
//! - a state-derived boost (awaiting-input and errors get amplified so a
//!   silent agent that needs attention still reads as "hot").
//!
//! The treemap step uses `compute_score` to size each tile; consumers that
//! want raw numbers (e.g. sparklines) read `bytes_per_sec` directly.
//!
//! Shape deliberately plain — no async in the type itself. The tracker task
//! (see `session_tracker`) pushes bytes into it; the HTTP handler reads a
//! snapshot out. Both sides lock the `HeatMap` mutex.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use tokio::sync::Mutex;

use atn_core::agent::AgentState;

/// Alpha for bytes/sec EWMA. Chosen for a ~30 s half-life at 1 Hz updates:
/// `half_life = ln(2) / alpha` ⇒ `alpha ≈ 0.023`.
const EWMA_ALPHA: f64 = 0.023;

/// Floor for the "activity" weight so a completely idle agent still has a
/// tiny visible tile. Keeps rank stable when everything is cold.
const ACTIVITY_FLOOR: f64 = 0.02;

/// Hard cap for the raw bytes/sec we feed into the score so one spammer
/// doesn't monopolize the entire viewport. 200 B/s is a lot of terminal
/// output; anything past that is bucketed the same.
const BYTES_SAT_RATE: f64 = 200.0;

#[derive(Clone, Debug)]
pub struct HeatState {
    /// Exponentially-weighted moving average of bytes-per-second.
    pub bytes_per_sec: f64,
    /// Total bytes observed since the tracker started. Exposed for tests
    /// and for a future debug endpoint.
    #[allow(dead_code)]
    pub total_bytes: u64,
    /// Last time we rolled the EWMA forward.
    pub last_update: Instant,
    /// When the agent's tracker was created. Exposed for tests and for a
    /// future debug endpoint ("how long since startup").
    #[allow(dead_code)]
    pub created_at: Instant,
}

impl HeatState {
    pub fn new(now: Instant) -> Self {
        Self {
            bytes_per_sec: 0.0,
            total_bytes: 0,
            last_update: now,
            created_at: now,
        }
    }

    /// Roll the EWMA forward to `now`, blending in `bytes_seen` over the
    /// elapsed interval. Safe to call with `bytes_seen == 0` to just decay.
    pub fn update(&mut self, bytes_seen: u64, now: Instant) {
        let dt = now.duration_since(self.last_update).as_secs_f64().max(0.001);
        let instantaneous = bytes_seen as f64 / dt;
        // Time-aware EWMA: larger dt ⇒ more weight on the new sample so
        // long quiet gaps decay the average toward 0 instead of "catching up".
        let effective_alpha = 1.0 - (1.0 - EWMA_ALPHA).powf(dt);
        self.bytes_per_sec =
            self.bytes_per_sec * (1.0 - effective_alpha) + instantaneous * effective_alpha;
        self.total_bytes = self.total_bytes.saturating_add(bytes_seen);
        self.last_update = now;
    }
}

/// Shared per-agent heat state. Keys are agent ids.
pub type HeatMap = Arc<Mutex<HashMap<String, HeatState>>>;

pub fn new_heat_map() -> HeatMap {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Spawn a task that feeds bytes from a session's output broadcast into the
/// shared `HeatMap`. Registers an initial `HeatState` for the agent before
/// returning so concurrent readers don't see a gap between "session exists"
/// and "heat entry exists".
///
/// Exits on channel close (session shut down) or on an explicit
/// `OutputSignal::Disconnected`, at which point the entry is dropped.
pub fn spawn_heat_tracker(
    mut rx: tokio::sync::broadcast::Receiver<atn_core::event::OutputSignal>,
    heat_map: HeatMap,
    agent_id: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        {
            let mut guard = heat_map.lock().await;
            guard.insert(agent_id.clone(), HeatState::new(Instant::now()));
        }

        loop {
            match rx.recv().await {
                Ok(atn_core::event::OutputSignal::Bytes(bytes)) => {
                    let now = Instant::now();
                    let mut guard = heat_map.lock().await;
                    if let Some(entry) = guard.get_mut(&agent_id) {
                        entry.update(bytes.len() as u64, now);
                    }
                }
                Ok(atn_core::event::OutputSignal::Disconnected) => {
                    heat_map.lock().await.remove(&agent_id);
                    break;
                }
                // Other signals (IdleDetected, PromptReady, QuestionDetected,
                // PushEvent) don't contribute bytes — just keep reading.
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    heat_map.lock().await.remove(&agent_id);
                    break;
                }
            }
        }
    })
}

/// Weight contributed by the current agent state. Callers add this to the
/// activity weight; the combination is then normalized into [0, 1].
pub fn state_boost(state: &AgentState) -> f64 {
    match state {
        AgentState::AwaitingHumanInput => 0.6,
        AgentState::Error { .. } => 0.7,
        AgentState::Blocked { .. } => 0.45,
        AgentState::CompletedTask => 0.15,
        AgentState::Disconnected => -0.35,
        AgentState::Starting | AgentState::Running | AgentState::Busy | AgentState::Idle => 0.0,
    }
}

/// Weight on the raw-activity term so a single spammer at max bytes/sec
/// doesn't peg the overall score to 1.0 — leaves headroom for state boosts
/// to re-rank awaiting-input / error tiles above a hot-but-idle one.
const ACTIVITY_WEIGHT: f64 = 0.7;

/// Final heat score, clamped to [0, 1]. Inputs:
/// - `heat`: the rolling `HeatState` for the agent
/// - `state`: the agent's current `AgentState`
///
/// Shape: weighted activity (bytes/sec saturated and down-weighted so bursts
/// alone can't peg the score) plus the state boost, with a floor so cold
/// agents don't collapse to zero area.
pub fn compute_score(heat: &HeatState, state: &AgentState) -> f64 {
    let activity = (heat.bytes_per_sec / BYTES_SAT_RATE).clamp(0.0, 1.0);
    let boosted = activity * ACTIVITY_WEIGHT + state_boost(state);
    let with_floor = boosted.max(ACTIVITY_FLOOR);
    with_floor.clamp(0.0, 1.0)
}

/// Wire shape returned by `GET /api/agents/heat`.
#[derive(Debug, Serialize)]
pub struct HeatInfo {
    pub id: String,
    /// Normalized score in [0, 1] — drives tile area in the treemap.
    pub heat: f64,
    /// Raw EWMA rate, bytes/sec. Drives the sparkline in compact tiles.
    pub bytes_per_sec: f64,
    /// The state-derived component that went into `heat`. UI can badge it.
    pub state_boost: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn assert_close(a: f64, b: f64, tol: f64) {
        assert!(
            (a - b).abs() < tol,
            "expected {a} ≈ {b} (tol {tol})"
        );
    }

    #[test]
    fn ewma_rises_with_bursts() {
        let t0 = Instant::now();
        let mut h = HeatState::new(t0);

        // 10 s of 100 B/s.
        for s in 1..=10 {
            h.update(100, t0 + Duration::from_secs(s));
        }

        assert!(
            h.bytes_per_sec > 20.0,
            "expected bytes_per_sec to climb, got {}",
            h.bytes_per_sec
        );
        assert!(h.bytes_per_sec <= 100.0);
        assert_eq!(h.total_bytes, 1000);
    }

    #[test]
    fn ewma_decays_when_quiet() {
        let t0 = Instant::now();
        let mut h = HeatState::new(t0);

        // Warm up.
        for s in 1..=30 {
            h.update(100, t0 + Duration::from_secs(s));
        }
        let hot = h.bytes_per_sec;
        assert!(hot > 50.0);

        // Then 5 minutes of silence.
        h.update(0, t0 + Duration::from_secs(30 + 5 * 60));
        assert!(
            h.bytes_per_sec < hot * 0.2,
            "expected heat to decay << {hot}, got {}",
            h.bytes_per_sec
        );
    }

    #[test]
    fn compute_score_blends_activity_and_state_boost() {
        let t0 = Instant::now();
        let mut hot = HeatState::new(t0);
        for s in 1..=60 {
            hot.update(300, t0 + Duration::from_secs(s));
        }
        // Saturate well past BYTES_SAT_RATE.
        let running_score = compute_score(&hot, &AgentState::Running);
        assert!(running_score > 0.5, "got {running_score}");

        let awaiting_score = compute_score(&hot, &AgentState::AwaitingHumanInput);
        assert!(
            awaiting_score > running_score,
            "awaiting-input should boost above running: {awaiting_score} vs {running_score}"
        );

        let disconnected_score = compute_score(&hot, &AgentState::Disconnected);
        assert!(
            disconnected_score < running_score,
            "disconnected should mute: {disconnected_score} vs {running_score}"
        );
    }

    #[test]
    fn compute_score_has_floor_for_quiet_agents() {
        let t0 = Instant::now();
        let cold = HeatState::new(t0);
        let score = compute_score(&cold, &AgentState::Idle);
        assert_close(score, ACTIVITY_FLOOR, 1e-9);
    }

    #[test]
    fn compute_score_clamps_to_unit_interval() {
        let t0 = Instant::now();
        let mut very_hot = HeatState::new(t0);
        for s in 1..=120 {
            very_hot.update(10_000, t0 + Duration::from_secs(s));
        }
        let score = compute_score(&very_hot, &AgentState::Error {
            message: "boom".to_string(),
        });
        assert!((0.0..=1.0).contains(&score), "got {score}");
    }

    #[test]
    fn new_heat_map_is_empty() {
        let m = new_heat_map();
        let guard = m.try_lock().unwrap();
        assert!(guard.is_empty());
    }
}
