use action_engine::{ActionRequest, validate_action};
use plugin_api::RepoSnapshot;
use state_store::StateStore;

fn main() {
    let request = ActionRequest {
        action: "repo.open".to_string(),
    };

    if !validate_action(&request) {
        eprintln!("invalid action");
        return;
    }

    let mut store = StateStore::new();
    store.set_repo(RepoSnapshot {
        root: ".".to_string(),
        head: None,
    });

    println!("{}", ui_shell::render_root(&store));
}
