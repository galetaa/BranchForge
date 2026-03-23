use plugin_api::ActionSpec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItem {
    pub action_id: String,
    pub title: String,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
}

pub fn build_palette(actions: &[ActionSpec], filter: &str, repo_open: bool) -> Vec<PaletteItem> {
    let needle = filter.to_lowercase();

    actions
        .iter()
        .filter(|spec| {
            if needle.is_empty() {
                true
            } else {
                spec.title.to_lowercase().contains(&needle)
            }
        })
        .map(|spec| PaletteItem {
            action_id: spec.action_id.clone(),
            title: spec.title.clone(),
            enabled: eval_when(spec.when.as_deref(), repo_open),
            disabled_reason: None,
        })
        .collect()
}

fn action_owner_plugin(action_id: &str) -> Option<&'static str> {
    if action_id == "repo.open" {
        return Some("repo_manager");
    }
    if action_id.starts_with("index.") || action_id.starts_with("commit.") {
        return Some("status");
    }
    if action_id.starts_with("history.") {
        return Some("history");
    }
    if action_id.starts_with("branch.") || action_id.starts_with("rebase.") {
        return Some("branches");
    }
    None
}

pub fn apply_plugin_health(
    items: &[PaletteItem],
    plugins: &[state_store::PluginStatus],
) -> Vec<PaletteItem> {
    items
        .iter()
        .cloned()
        .map(|mut item| {
            let Some(owner) = action_owner_plugin(&item.action_id) else {
                return item;
            };

            let unavailable = plugins.iter().find(|status| {
                status.plugin_id == owner
                    && matches!(status.health, state_store::PluginHealth::Unavailable { .. })
            });
            if let Some(status) = unavailable {
                let reason = match &status.health {
                    state_store::PluginHealth::Unavailable { message } => message.clone(),
                    state_store::PluginHealth::Ready => String::new(),
                };
                item.enabled = false;
                item.disabled_reason = Some(format!("plugin {} unavailable: {}", owner, reason));
            }

            item
        })
        .collect()
}

fn eval_when(expr: Option<&str>, repo_open: bool) -> bool {
    match expr {
        None => true,
        Some("always") => true,
        Some("repo.is_open") => repo_open,
        Some(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(action_id: &str, title: &str, when: Option<&str>) -> ActionSpec {
        ActionSpec {
            action_id: action_id.to_string(),
            title: title.to_string(),
            when: when.map(ToString::to_string),
            params_schema: None,
            danger: None,
        }
    }

    #[test]
    fn filters_by_title_and_respects_when() {
        let items = build_palette(
            &[
                action("repo.open", "Open Repository", Some("always")),
                action("commit.create", "Commit", Some("repo.is_open")),
            ],
            "commit",
            false,
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].action_id, "commit.create");
        assert!(!items[0].enabled);
        assert!(items[0].disabled_reason.is_none());
    }

    #[test]
    fn disables_action_when_owner_plugin_unavailable() {
        let items = vec![PaletteItem {
            action_id: "commit.create".to_string(),
            title: "Commit".to_string(),
            enabled: true,
            disabled_reason: None,
        }];

        let adjusted = apply_plugin_health(
            &items,
            &[state_store::PluginStatus {
                plugin_id: "status".to_string(),
                health: state_store::PluginHealth::Unavailable {
                    message: "restarting".to_string(),
                },
            }],
        );

        assert!(!adjusted[0].enabled);
        assert_eq!(
            adjusted[0].disabled_reason.as_deref(),
            Some("plugin status unavailable: restarting")
        );
    }
}
