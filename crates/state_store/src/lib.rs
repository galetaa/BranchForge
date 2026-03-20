use plugin_api::RepoSnapshot;

#[derive(Debug, Clone, Default)]
pub struct StateStore {
    current_repo: Option<RepoSnapshot>,
}

impl StateStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_repo(&mut self, repo: RepoSnapshot) {
        self.current_repo = Some(repo);
    }

    pub fn repo(&self) -> Option<&RepoSnapshot> {
        self.current_repo.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_repo_snapshot() {
        let mut store = StateStore::new();
        store.set_repo(RepoSnapshot {
            root: "./demo".to_string(),
            head: Some("main".to_string()),
        });

        let head = store.repo().and_then(|repo| repo.head.as_deref());
        assert_eq!(head, Some("main"));
    }
}
