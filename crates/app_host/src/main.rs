use action_engine::{ActionRequest, validate_action};
use app_host::{run_action_roundtrip, run_ui_state_smoke};
use plugin_api::RepoSnapshot;
use state_store::StateStore;

fn main() {
    let roundtrip = run_action_roundtrip("repo.open");
    if let Ok(action_id) = roundtrip {
        println!("runtime roundtrip ok for action: {action_id}");
    }
    println!("{}", run_ui_state_smoke());

    let request = ActionRequest {
        action: "repo.open".to_string(),
    };

    if !validate_action(&request) {
        eprintln!("invalid action");
        return;
    }

    let mut store = StateStore::new();
    store.update_repo(RepoSnapshot {
        root: ".".to_string(),
        head: None,
    });

    println!("{}", ui_shell::render_root(&store));
}
