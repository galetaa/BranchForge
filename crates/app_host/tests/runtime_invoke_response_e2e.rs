use app_host::run_action_roundtrip;

#[test]
fn app_host_runtime_roundtrip_happy_path() {
    let result = run_action_roundtrip("repo.open");
    assert!(result.is_ok());

    if let Ok(action_id) = result {
        assert_eq!(action_id, "repo.open");
    }
}
