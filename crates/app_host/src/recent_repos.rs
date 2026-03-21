use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct RecentReposStorage {
    file_path: PathBuf,
    max_items: usize,
}

impl RecentReposStorage {
    pub fn new(file_path: PathBuf, max_items: usize) -> Self {
        Self {
            file_path,
            max_items,
        }
    }

    pub fn add_recent(&self, repo_path: &str) -> Result<Vec<String>, String> {
        let mut items = self.load().unwrap_or_default();
        items.retain(|item| item != repo_path);
        items.insert(0, repo_path.to_string());
        if items.len() > self.max_items {
            items.truncate(self.max_items);
        }
        self.save(&items)?;
        Ok(items)
    }

    pub fn load(&self) -> Result<Vec<String>, String> {
        if !self.file_path.exists() {
            return Ok(Vec::new());
        }

        let raw = std::fs::read_to_string(&self.file_path).map_err(|e| e.to_string())?;
        Ok(raw
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToString::to_string)
            .collect())
    }

    fn save(&self, items: &[String]) -> Result<(), String> {
        if let Some(parent) = self.file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let body = items.join("\n");
        std::fs::write(&self.file_path, body).map_err(|e| e.to_string())
    }
}

pub fn default_recent_repos_file() -> PathBuf {
    std::env::temp_dir().join("branchforge/recent_repos.txt")
}

pub fn default_recent_repos_storage() -> RecentReposStorage {
    RecentReposStorage::new(default_recent_repos_file(), 10)
}

pub fn persist_recent_repo(repo_path: &Path) -> Result<Vec<String>, String> {
    let storage = default_recent_repos_storage();
    storage.add_recent(&repo_path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_file() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("branchforge/recent-repos-{nanos}.txt"))
    }

    #[test]
    fn stores_recent_paths_without_duplicates() {
        let file = test_file();
        let storage = RecentReposStorage::new(file.clone(), 3);

        assert!(storage.add_recent("/tmp/repo-a").is_ok());
        assert!(storage.add_recent("/tmp/repo-b").is_ok());
        let result = storage.add_recent("/tmp/repo-a");
        assert!(result.is_ok());

        if let Ok(items) = result {
            assert_eq!(items[0], "/tmp/repo-a");
            assert_eq!(items[1], "/tmp/repo-b");
            assert_eq!(items.len(), 2);
        }

        let _ = std::fs::remove_file(file);
    }
}
