use std::path::{Path, PathBuf};

use async_trait::async_trait;
use wiki_common::async_storage::AsyncWikiStorage;
use wiki_common::model::WikiPage;

/// File-based wiki storage: each page is a `.md` file in a directory.
pub struct FileWikiStorage {
    dir: PathBuf,
}

impl FileWikiStorage {
    pub fn new(dir: &Path) -> Self {
        std::fs::create_dir_all(dir).expect("failed to create wiki directory");
        Self {
            dir: dir.to_path_buf(),
        }
    }

    fn page_path(&self, title: &str) -> PathBuf {
        let safe_name = sanitize_title(title);
        self.dir.join(format!("{safe_name}.md"))
    }

    fn meta_path(&self, title: &str) -> PathBuf {
        let safe_name = sanitize_title(title);
        self.dir.join(format!("{safe_name}.meta.json"))
    }
}

#[async_trait]
impl AsyncWikiStorage for FileWikiStorage {
    async fn get_page(&self, title: &str) -> Option<WikiPage> {
        let path = self.page_path(title);
        let content = tokio::fs::read_to_string(&path).await.ok()?;

        let meta_path = self.meta_path(title);
        let (created_at, updated_at) = if let Ok(meta_json) = tokio::fs::read_to_string(&meta_path).await {
            if let Ok(meta) = serde_json::from_str::<PageMeta>(&meta_json) {
                (meta.created_at, meta.updated_at)
            } else {
                (0, 0)
            }
        } else {
            (0, 0)
        };

        Some(WikiPage {
            title: title.to_string(),
            content,
            created_at,
            updated_at,
        })
    }

    async fn save_page(&self, page: WikiPage) {
        let path = self.page_path(&page.title);

        // Create parent directories for nested page titles (e.g., "Coordination/Goals").
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        let _ = tokio::fs::write(&path, &page.content).await;

        let meta = PageMeta {
            created_at: page.created_at,
            updated_at: page.updated_at,
        };
        let meta_path = self.meta_path(&page.title);
        if let Some(parent) = meta_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        if let Ok(json) = serde_json::to_string(&meta) {
            let _ = tokio::fs::write(&meta_path, json).await;
        }
    }

    async fn delete_page(&self, title: &str) {
        let _ = tokio::fs::remove_file(self.page_path(title)).await;
        let _ = tokio::fs::remove_file(self.meta_path(title)).await;
    }

    async fn list_pages(&self) -> Vec<String> {
        let mut pages = Vec::new();
        if let Ok(mut entries) = tokio::fs::read_dir(&self.dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".md") && !name.ends_with(".meta.json") {
                    pages.push(name.trim_end_matches(".md").to_string());
                }
            }
        }
        pages.sort();
        pages
    }

    async fn has_page(&self, title: &str) -> bool {
        self.page_path(title).exists()
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PageMeta {
    created_at: u64,
    updated_at: u64,
}

/// Sanitize a page title for use as a filename.
/// Replaces `/` with `__` and keeps only alphanumeric, `-`, `_`.
fn sanitize_title(title: &str) -> String {
    title
        .replace('/', "__")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_simple() {
        assert_eq!(sanitize_title("MainPage"), "MainPage");
    }

    #[test]
    fn sanitize_with_slash() {
        assert_eq!(sanitize_title("Coordination/Goals"), "Coordination__Goals");
    }

    #[test]
    fn sanitize_strips_special() {
        assert_eq!(sanitize_title("page name!@#"), "pagename");
    }

    #[tokio::test]
    async fn save_and_get() {
        let dir = tempfile::tempdir().unwrap();
        let storage = FileWikiStorage::new(dir.path());

        let page = WikiPage::new("TestPage", "Hello, wiki!", 1000);
        storage.save_page(page.clone()).await;

        let got = storage.get_page("TestPage").await.unwrap();
        assert_eq!(got.title, "TestPage");
        assert_eq!(got.content, "Hello, wiki!");
        assert_eq!(got.created_at, 1000);
    }

    #[tokio::test]
    async fn list_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let storage = FileWikiStorage::new(dir.path());

        storage
            .save_page(WikiPage::new("A", "aaa", 1))
            .await;
        storage
            .save_page(WikiPage::new("B", "bbb", 2))
            .await;

        let pages = storage.list_pages().await;
        assert_eq!(pages.len(), 2);

        storage.delete_page("A").await;
        assert!(!storage.has_page("A").await);
        assert_eq!(storage.list_pages().await.len(), 1);
    }
}
