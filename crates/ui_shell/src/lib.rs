use state_store::StateStore;

pub fn render_root(store: &StateStore) -> String {
    match store.repo() {
        Some(repo) => format!("Repo: {}", repo.root),
        None => "Repo: <not opened>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_empty_state() {
        let store = StateStore::new();
        assert_eq!(render_root(&store), "Repo: <not opened>");
    }
}
