use crate::thread::session_selector_preferences::{SelectorLayoutEntry, SelectorLayoutPreferences};
use crate::thread::{SessionConfigOption, SessionConfigSelectGroup};

pub(super) fn selector_name(
    layout: &SelectorLayoutPreferences,
    selector_id: &str,
    default_name: &str,
) -> String {
    layout
        .entry(selector_id)
        .and_then(|entry| entry.name.as_deref())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(default_name)
        .to_string()
}

fn selector_visible(layout: &SelectorLayoutPreferences, selector_id: &str) -> bool {
    layout
        .entry(selector_id)
        .and_then(|entry| entry.visible)
        .unwrap_or(true)
}

pub(super) fn apply_group_layout(
    layout: &SelectorLayoutPreferences,
    selector_id: &str,
    groups: Vec<SessionConfigSelectGroup>,
) -> Vec<SessionConfigSelectGroup> {
    let Some(SelectorLayoutEntry {
        groups: Some(group_order),
        ..
    }) = layout.entry(selector_id)
    else {
        return groups;
    };
    let mut remaining = groups;
    let mut ordered = Vec::new();

    for group_id in group_order {
        if let Some(index) = remaining
            .iter()
            .position(|group| group.group.0.as_ref() == group_id.as_str())
        {
            ordered.push(remaining.remove(index));
        }
    }

    if ordered.is_empty() {
        remaining
    } else {
        ordered
    }
}

pub(super) fn apply_selector_order(
    layout: &SelectorLayoutPreferences,
    options: Vec<(&'static str, SessionConfigOption)>,
) -> Vec<SessionConfigOption> {
    let mut remaining = options
        .into_iter()
        .filter(|(selector_id, _)| selector_visible(layout, selector_id))
        .collect::<Vec<_>>();
    let mut ordered = Vec::with_capacity(remaining.len());

    if let Some(selector_order) = &layout.order {
        for selector_id in selector_order {
            if let Some(index) = remaining
                .iter()
                .position(|(candidate_id, _)| *candidate_id == selector_id.as_str())
            {
                ordered.push(remaining.remove(index).1);
            }
        }
    }

    ordered.extend(remaining.into_iter().map(|(_, option)| option));
    ordered
}
