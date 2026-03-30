use wiki_common::async_storage::AsyncWikiStorage;
use wiki_common::model::WikiPage;

/// Well-known coordination page titles.
pub const GOALS_PAGE: &str = "Coordination/Goals";
pub const AGENTS_PAGE: &str = "Coordination/Agents";
pub const REQUESTS_PAGE: &str = "Coordination/Requests";
pub const BLOCKERS_PAGE: &str = "Coordination/Blockers";
pub const LOG_PAGE: &str = "Coordination/Log";

/// Seed the default coordination pages if they don't already exist.
pub async fn seed_coordination_pages(storage: &dyn AsyncWikiStorage, now: u64) {
    let seeds = [
        (GOALS_PAGE, "# Goals\n\nDefine overall project objectives here.\n"),
        (AGENTS_PAGE, "# Agents\n\nWho is working on what.\n"),
        (REQUESTS_PAGE, "# Requests\n\nOpen feature and bug fix requests.\n"),
        (BLOCKERS_PAGE, "# Blockers\n\nCurrent blockers and dependency chain.\n"),
        (LOG_PAGE, "# Coordination Log\n\n"),
    ];

    for (title, content) in seeds {
        if !storage.has_page(title).await {
            storage.save_page(WikiPage::new(title, content, now)).await;
        }
    }
}

/// Append a timestamped entry to the coordination log page.
pub async fn append_log(
    storage: &dyn AsyncWikiStorage,
    entry: &str,
    timestamp: &str,
    now: u64,
) {
    let mut page = storage
        .get_page(LOG_PAGE)
        .await
        .unwrap_or_else(|| WikiPage::new(LOG_PAGE, "# Coordination Log\n\n", now));

    page.content.push_str(&format!("- [{timestamp}] {entry}\n"));
    page.updated_at = now;
    storage.save_page(page).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::FileWikiStorage;

    #[tokio::test]
    async fn seed_creates_pages() {
        let dir = tempfile::tempdir().unwrap();
        let storage = FileWikiStorage::new(dir.path());

        seed_coordination_pages(&storage, 1000).await;

        assert!(storage.has_page(GOALS_PAGE).await);
        assert!(storage.has_page(AGENTS_PAGE).await);
        assert!(storage.has_page(REQUESTS_PAGE).await);
        assert!(storage.has_page(BLOCKERS_PAGE).await);
        assert!(storage.has_page(LOG_PAGE).await);
    }

    #[tokio::test]
    async fn seed_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let storage = FileWikiStorage::new(dir.path());

        seed_coordination_pages(&storage, 1000).await;

        // Modify a page.
        let mut page = storage.get_page(GOALS_PAGE).await.unwrap();
        page.content = "# Goals\n\nCustom content.\n".to_string();
        storage.save_page(page).await;

        // Re-seed should not overwrite.
        seed_coordination_pages(&storage, 2000).await;
        let page = storage.get_page(GOALS_PAGE).await.unwrap();
        assert!(page.content.contains("Custom content"));
    }

    #[tokio::test]
    async fn append_log_entry() {
        let dir = tempfile::tempdir().unwrap();
        let storage = FileWikiStorage::new(dir.path());
        seed_coordination_pages(&storage, 1000).await;

        append_log(&storage, "Agent A started", "2026-03-29T14:00:00Z", 1001).await;
        append_log(&storage, "Agent B started", "2026-03-29T14:01:00Z", 1002).await;

        let page = storage.get_page(LOG_PAGE).await.unwrap();
        assert!(page.content.contains("Agent A started"));
        assert!(page.content.contains("Agent B started"));
    }
}
