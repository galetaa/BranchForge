use crate::palette::PaletteItem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowLayout {
    pub left_slot: String,
    pub service_area: String,
    pub diff_area: Option<String>,
    pub active_view: Option<String>,
}

pub fn build_layout(
    status_panel: &str,
    palette: &[PaletteItem],
    diff_area: Option<String>,
    active_view: Option<String>,
) -> WindowLayout {
    let palette_line = if palette.is_empty() {
        "palette: <empty>".to_string()
    } else {
        let labels = palette
            .iter()
            .map(|item| {
                format!(
                    "{} ({})",
                    item.title,
                    if item.enabled { "on" } else { "off" }
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("palette: {labels}")
    };

    WindowLayout {
        left_slot: status_panel.to_string(),
        service_area: palette_line,
        diff_area,
        active_view,
    }
}

pub fn render_layout(layout: &WindowLayout) -> String {
    let active = layout
        .active_view
        .as_ref()
        .map_or_else(|| "<none>".to_string(), Clone::clone);
    let diff = layout
        .diff_area
        .as_ref()
        .map_or_else(|| "".to_string(), |text| format!("[diff]\n{text}\n"));

    format!(
        "[window]\nactive_view: {active}\n[left-slot]\n{}\n[service]\n{}\n{diff}",
        layout.left_slot, layout.service_area
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_window_sections() {
        let layout = build_layout(
            "status panel text",
            &[PaletteItem {
                action_id: "repo.open".to_string(),
                title: "Open Repository".to_string(),
                enabled: true,
            }],
            None,
            Some("status.panel".to_string()),
        );

        let rendered = render_layout(&layout);
        assert!(rendered.contains("[left-slot]"));
        assert!(rendered.contains("[service]"));
        assert!(rendered.contains("active_view: status.panel"));
    }
}
