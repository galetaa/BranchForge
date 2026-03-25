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
        plugin_id: "branches".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
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
            spec(
                "rebase.plan.create",
                "Create Rebase Plan",
                Some(DangerLevel::Medium),
                ActionEffects::read_only(),
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "rebase.execute",
                "Execute Rebase Plan",
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
            spec(
                "rebase.continue",
                "Continue Rebase",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "rebase.skip",
                "Skip Rebase Commit",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "rebase.abort",
                "Abort Rebase",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "merge.execute",
                "Merge Branch",
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
            spec(
                "merge.abort",
                "Abort Merge",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "reset.soft",
                "Reset --soft",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "reset.mixed",
                "Reset --mixed",
                Some(DangerLevel::High),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
            spec(
                "reset.hard",
                "Reset --hard",
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
