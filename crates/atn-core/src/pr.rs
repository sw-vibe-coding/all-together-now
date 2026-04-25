//! Pull-request records for the git-sync-agents saga.
//!
//! `atn-syncd` writes one `PrRecord` per detected `.atn-ready-to-pr`
//! marker; the atn-server REST layer reads them, the dashboard +
//! atn-cli render + mutate them. Lives in `atn-core` so every
//! crate that touches a PR file uses the same shape.
//!
//! The on-disk layout is `<prs-dir>/<id>.json` where the id is
//! `<agent-id>-<branch>-<short-sha>` — sortable, unique enough for
//! a single host's daily traffic.

use serde::{Deserialize, Serialize};

/// PR lifecycle. Append-only — once `Merged` or `Rejected`, the
/// record is never reset. New PRs land as `Open`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrStatus {
    Open,
    Merged,
    Rejected,
}

/// One PR-equivalent record.
///
/// `commit` is the source-side SHA the syncd resolved at push time
/// (always populated). `merge_commit` is the central-side SHA the
/// merge produced (set only on a successful merge). `merged_at` /
/// `rejected_at` track the lifecycle transitions.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrRecord {
    pub id: String,
    pub agent_id: String,
    pub source_repo: String,
    pub branch: String,
    pub target: String,
    pub commit: String,
    pub summary: String,
    pub status: PrStatus,
    pub created_at: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected_at: Option<String>,
    /// Optional stderr / message captured at the lifecycle boundary
    /// (e.g. push or merge). Useful when scraping the JSON outside
    /// of the dashboard.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl PrRecord {
    /// Build the canonical filename for a PR record.
    pub fn filename(&self) -> String {
        format!("{}.json", self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PrRecord {
        PrRecord {
            id: "alice-feature-x-abcdef0".into(),
            agent_id: "alice".into(),
            source_repo: "/tmp/alice".into(),
            branch: "feature-x".into(),
            target: "main".into(),
            commit: "abcdef0123456789".into(),
            summary: "feature x ready for review".into(),
            status: PrStatus::Open,
            created_at: "2026-04-24T10:00:00Z".into(),
            merge_commit: None,
            merged_at: None,
            rejected_at: None,
            last_error: None,
        }
    }

    #[test]
    fn pr_record_serde_roundtrip() {
        let pr = sample();
        let json = serde_json::to_string_pretty(&pr).unwrap();
        let back: PrRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back, pr);
        assert_eq!(back.status, PrStatus::Open);
    }

    #[test]
    fn merged_record_carries_merge_metadata() {
        let mut pr = sample();
        pr.status = PrStatus::Merged;
        pr.merge_commit = Some("0011223344".into());
        pr.merged_at = Some("2026-04-24T11:00:00Z".into());
        let json = serde_json::to_string_pretty(&pr).unwrap();
        assert!(json.contains("\"status\": \"merged\""));
        assert!(json.contains("\"merge_commit\""));
        let back: PrRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, PrStatus::Merged);
        assert_eq!(back.merge_commit.as_deref(), Some("0011223344"));
    }

    #[test]
    fn rejected_record_carries_rejected_at() {
        let mut pr = sample();
        pr.status = PrStatus::Rejected;
        pr.rejected_at = Some("2026-04-24T11:30:00Z".into());
        pr.last_error = Some("scope creep".into());
        let json = serde_json::to_string(&pr).unwrap();
        assert!(json.contains("\"status\":\"rejected\""));
        let back: PrRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.status, PrStatus::Rejected);
        assert_eq!(back.last_error.as_deref(), Some("scope creep"));
    }

    #[test]
    fn pr_status_serializes_snake_case() {
        let cases = [
            (PrStatus::Open, "\"open\""),
            (PrStatus::Merged, "\"merged\""),
            (PrStatus::Rejected, "\"rejected\""),
        ];
        for (s, want) in cases {
            assert_eq!(serde_json::to_string(&s).unwrap(), want);
        }
    }

    #[test]
    fn record_filename_uses_id() {
        let pr = sample();
        assert_eq!(pr.filename(), "alice-feature-x-abcdef0.json");
    }

    #[test]
    fn record_omits_optional_fields_when_none() {
        // Compact JSON shape for an Open PR — should be lean.
        let pr = sample();
        let json = serde_json::to_string(&pr).unwrap();
        assert!(!json.contains("merge_commit"));
        assert!(!json.contains("merged_at"));
        assert!(!json.contains("rejected_at"));
        assert!(!json.contains("last_error"));
    }
}
