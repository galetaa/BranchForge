use plugin_api::ActionSpec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItem {
    pub action_id: String,
    pub title: String,
    pub enabled: bool,
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
    }
}
