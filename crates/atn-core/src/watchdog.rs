//! Watchdog configuration — static thresholds for per-agent stall
//! detection. The runtime state that pairs with this config (the
//! rolling `last_output_at` / `stalled` flag) lives in
//! `atn_pty::watchdog` since it touches PTY timing.

use serde::{Deserialize, Serialize};

/// Seconds of silence while the agent is `running` before the
/// watchdog flags it as stalled.
pub const DEFAULT_STALL_SECS: u64 = 60;

/// Per-agent watchdog thresholds.
///
/// Embedded in `SpawnSpec` as an optional `[watchdog]` TOML section.
/// Defaults apply when absent.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WatchdogConfig {
    /// Flag the agent as `stalled` after this many seconds of silence
    /// while its state is `running`.
    #[serde(default = "default_stall_secs")]
    pub stall_secs: u64,
    /// Optional hard ceiling on continuous `running` time — step 6's
    /// action layer posts a `blocked_notice` once exceeded. `None`
    /// disables the ceiling.
    #[serde(default)]
    pub max_running_secs: Option<u64>,
}

fn default_stall_secs() -> u64 {
    DEFAULT_STALL_SECS
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            stall_secs: DEFAULT_STALL_SECS,
            max_running_secs: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_expected_values() {
        let c = WatchdogConfig::default();
        assert_eq!(c.stall_secs, 60);
        assert!(c.max_running_secs.is_none());
    }

    #[test]
    fn toml_roundtrip_accepts_empty_section() {
        // An empty `[watchdog]` block should deserialize to defaults.
        let toml = "";
        let c: WatchdogConfig = toml::from_str(toml).unwrap();
        assert_eq!(c, WatchdogConfig::default());
    }

    #[test]
    fn toml_roundtrip_accepts_partial() {
        let c: WatchdogConfig = toml::from_str("stall_secs = 30").unwrap();
        assert_eq!(c.stall_secs, 30);
        assert!(c.max_running_secs.is_none());
    }

    #[test]
    fn toml_roundtrip_accepts_full() {
        let c: WatchdogConfig =
            toml::from_str("stall_secs = 45\nmax_running_secs = 900").unwrap();
        assert_eq!(c.stall_secs, 45);
        assert_eq!(c.max_running_secs, Some(900));
    }
}
