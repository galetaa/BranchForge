use plugin_api::{
    ActionEffects, ActionSpec, ConfirmPolicy, DangerLevel, PluginHello, PluginRegister, RpcRequest,
    ViewSpec,
};

fn spec(
    action_id: &str,
    title: &str,
    danger: Option<DangerLevel>,
    effects: ActionEffects,
    confirm_policy: ConfirmPolicy,
) -> ActionSpec {
    ActionSpec {
        action_id: action_id.to_string(),
        title: title.to_string(),
        when: Some("repo.is_open".to_string()),
        params_schema: None,
        danger,
        effects,
        confirm_policy,
    }
}

fn build_hello_request() -> RpcRequest {
    PluginHello {
        plugin_id: "status".to_string(),
        version: "0.1".to_string(),
    }
    .to_request("hello-1")
}

fn build_register_request() -> RpcRequest {
    PluginRegister {
        actions: vec![
            spec(
                "index.stage_selected",
                "Stage Selected",
                None,
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "index.unstage_selected",
                "Unstage Selected",
                None,
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "index.stage_hunk",
                "Stage Hunk",
                None,
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "index.unstage_hunk",
                "Unstage Hunk",
                None,
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "commit.create",
                "Commit",
                None,
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "commit.amend",
                "Amend Commit",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "file.discard",
                "Discard File Changes",
                Some(DangerLevel::High),
                ActionEffects {
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
        ],
        views: vec![ViewSpec {
            view_id: "status.panel".to_string(),
            title: "Status".to_string(),
            slot: "left".to_string(),
            when: Some("repo.is_open".to_string()),
        }],
    }
    .to_request("register-1")
}

fn main() {
    let hello = build_hello_request();
    let register = build_register_request();

    println!("{} -> {}", hello.method, register.method);
}
