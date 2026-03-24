use plugin_api::{
    ActionEffects, ActionSpec, ConfirmPolicy, DangerLevel, PluginHello, PluginRegister,
    RpcRequest, ViewSpec,
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
        plugin_id: "branches".to_string(),
        version: "0.1".to_string(),
    }
    .to_request("hello-1")
}

fn build_register_request() -> RpcRequest {
    let rebase_beta = rebase_beta_enabled();
    PluginRegister {
        actions: vec![
            spec(
                "branch.checkout",
                "Checkout Branch",
                None,
                ActionEffects {
                    writes_refs: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "branch.create",
                "Create Branch",
                None,
                ActionEffects {
                    writes_refs: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "branch.rename",
                "Rename Branch",
                None,
                ActionEffects {
                    writes_refs: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "branch.delete",
                "Delete Branch",
                Some(DangerLevel::High),
                ActionEffects::mutating_refs(),
                ConfirmPolicy::Always,
            ),
            spec(
                "rebase.interactive",
                "Interactive Rebase (beta)",
                Some(DangerLevel::High),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
        ]
        .into_iter()
        .filter(|spec| {
            if spec.action_id == "rebase.interactive" {
                rebase_beta
            } else {
                true
            }
        })
        .collect(),
        views: vec![ViewSpec {
            view_id: "branches.panel".to_string(),
            title: "Branches".to_string(),
            slot: "left".to_string(),
            when: Some("repo.is_open".to_string()),
        }],
    }
    .to_request("register-1")
}

fn rebase_beta_enabled() -> bool {
    matches!(std::env::var("BRANCHFORGE_REBASE_BETA").as_deref(), Ok("1"))
}

fn main() {
    let hello = build_hello_request();
    let register = build_register_request();

    println!("{} -> {}", hello.method, register.method);
}
