use crate::defaults::{default_keys, default_positions, default_stat_positions};
use crate::state::tab_metadata::{normalize_bar_count, normalize_tab_order};
use crate::{models::*, state::plugin::*};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeResetKind {
    Default,
    Custom,
}

pub struct CustomTabDeletePlan {
    pub custom_tabs: Vec<CustomTab>,
    pub tab_order: Vec<String>,
    pub next_selected: String,
}

pub fn zeroed_counters(keys: &KeyMappings) -> KeyCounters {
    keys.iter()
        .map(|(mode, mode_keys)| {
            (
                mode.clone(),
                mode_keys.iter().map(|key| (key.canonical(), 0)).collect(),
            )
        })
        .collect()
}

pub fn reset_all_editor_data(
    store: &mut AppStoreData,
    keys: &KeyMappings,
    positions: &KeyPositions,
    stat_positions: &StatPositions,
) {
    store.keys = keys.clone();
    store.key_positions = positions.clone();
    store.stat_positions = stat_positions.clone();
    store.graph_positions.clear();
    store.knob_positions.clear();
    store.sprite_positions.clear();
    store.layer_groups.clear();
    store.key_counters = zeroed_counters(keys);
    store.custom_tabs.clear();
    store.tab_order = normalize_tab_order(&[], &store.custom_tabs);
    store.bar_count = normalize_bar_count(crate::models::default_bar_count(), &store.tab_order);
    store.selected_key_type = "4key".to_string();
    store.tab_note_overrides.clear();
    store.tab_css_overrides.clear();
    crate::state::native_element_id::rekey_store_element_ids(store);
}

pub fn reset_mode_kind(store: &AppStoreData, mode: &str) -> Option<ModeResetKind> {
    if default_keys().contains_key(mode) {
        Some(ModeResetKind::Default)
    } else if store.custom_tabs.iter().any(|tab| tab.id == mode) {
        Some(ModeResetKind::Custom)
    } else {
        None
    }
}

pub fn is_selectable_mode(store: &AppStoreData, mode: &str) -> bool {
    default_keys().contains_key(mode)
        || (store.keys.contains_key(mode) && store.custom_tabs.iter().any(|tab| tab.id == mode))
}

pub fn select_mode_if_available(store: &mut AppStoreData, requested: &str) -> (bool, String) {
    if !is_selectable_mode(store, requested) {
        return (false, store.selected_key_type.clone());
    }
    store.selected_key_type = requested.to_string();
    (true, store.selected_key_type.clone())
}

pub fn apply_reset_mode_data(store: &mut AppStoreData, mode: &str, kind: ModeResetKind) {
    match kind {
        ModeResetKind::Default => {
            if let Some(keys) = default_keys().get(mode) {
                store.keys.insert(mode.to_string(), keys.clone());
            }
            if let Some(positions) = default_positions().get(mode) {
                store
                    .key_positions
                    .insert(mode.to_string(), positions.clone());
            }
            if let Some(positions) = default_stat_positions().get(mode) {
                store
                    .stat_positions
                    .insert(mode.to_string(), positions.clone());
            }
        }
        ModeResetKind::Custom => {
            store.keys.insert(mode.to_string(), Vec::new());
            store.key_positions.insert(mode.to_string(), Vec::new());
            store.stat_positions.insert(mode.to_string(), Vec::new());
        }
    }

    store.graph_positions.insert(mode.to_string(), Vec::new());
    store.knob_positions.insert(mode.to_string(), Vec::new());
    store.sprite_positions.insert(mode.to_string(), Vec::new());
    store.layer_groups.remove(mode);
    store.tab_css_overrides.remove(mode);
    store.tab_note_overrides.remove(mode);

    let mode_keys = store.keys.get(mode).cloned().unwrap_or_default();
    store.key_counters.insert(
        mode.to_string(),
        mode_keys
            .into_iter()
            .map(|key| (key.canonical(), 0))
            .collect(),
    );
}

pub fn mode_positions_equal_without_ids(
    current: &AppStoreData,
    candidate: &AppStoreData,
    mode: &str,
) -> bool {
    let mut current_keys = current.key_positions.get(mode).cloned();
    let mut candidate_keys = candidate.key_positions.get(mode).cloned();
    for position in current_keys.iter_mut().flatten() {
        position.id.clear();
    }
    for position in candidate_keys.iter_mut().flatten() {
        position.id.clear();
    }

    let mut current_stats = current.stat_positions.get(mode).cloned();
    let mut candidate_stats = candidate.stat_positions.get(mode).cloned();
    for position in current_stats.iter_mut().flatten() {
        position.position.id.clear();
    }
    for position in candidate_stats.iter_mut().flatten() {
        position.position.id.clear();
    }

    let mut current_graphs = current.graph_positions.get(mode).cloned();
    let mut candidate_graphs = candidate.graph_positions.get(mode).cloned();
    for position in current_graphs.iter_mut().flatten() {
        position.position.id.clear();
    }
    for position in candidate_graphs.iter_mut().flatten() {
        position.position.id.clear();
    }

    let mut current_knobs = current.knob_positions.get(mode).cloned();
    let mut candidate_knobs = candidate.knob_positions.get(mode).cloned();
    for position in current_knobs.iter_mut().flatten() {
        position.position.id.clear();
    }
    for position in candidate_knobs.iter_mut().flatten() {
        position.position.id.clear();
    }

    let mut current_sprites = current.sprite_positions.get(mode).cloned();
    let mut candidate_sprites = candidate.sprite_positions.get(mode).cloned();
    for sprite in current_sprites
        .iter_mut()
        .chain(candidate_sprites.iter_mut())
        .flatten()
    {
        sprite.id.clear();
        for pose in &mut sprite.poses {
            pose.pose_id.clear();
        }
    }

    current_keys == candidate_keys
        && optional_collections_equal(current_stats.as_deref(), candidate_stats.as_deref())
        && optional_collections_equal(current_graphs.as_deref(), candidate_graphs.as_deref())
        && optional_collections_equal(current_knobs.as_deref(), candidate_knobs.as_deref())
        && optional_collections_equal(current_sprites.as_deref(), candidate_sprites.as_deref())
}

pub fn optional_collections_equal<T: PartialEq>(
    current: Option<&[T]>,
    candidate: Option<&[T]>,
) -> bool {
    current.unwrap_or_default() == candidate.unwrap_or_default()
}

pub fn mode_reset_semantics_equal(
    current: &AppStoreData,
    candidate: &AppStoreData,
    mode: &str,
) -> bool {
    current.keys.get(mode) == candidate.keys.get(mode)
        && mode_positions_equal_without_ids(current, candidate, mode)
        && optional_collections_equal(
            current.layer_groups.get(mode).map(Vec::as_slice),
            candidate.layer_groups.get(mode).map(Vec::as_slice),
        )
        && current.tab_css_overrides.get(mode) == candidate.tab_css_overrides.get(mode)
        && current.tab_note_overrides.get(mode) == candidate.tab_note_overrides.get(mode)
        && current.key_counters.get(mode) == candidate.key_counters.get(mode)
}

pub fn has_plugin_instances_in_mode(store: &AppStoreData, mode: &str) -> bool {
    let mut found = false;
    for_each_stored_plugin_instances(store, |_, instances| {
        found |= instances
            .iter()
            .any(|instance| normalize_plugin_instance_tab_id(instance.tab_id.as_deref()) == mode);
    });
    found
}

pub fn reset_mode_data(store: &mut AppStoreData, mode: &str, kind: ModeResetKind) -> bool {
    let plugin_instances_changed = has_plugin_instances_in_mode(store, mode);
    let mut candidate = store.clone();
    apply_reset_mode_data(&mut candidate, mode, kind);
    crate::state::migration::canonicalize_gradient_pairs(&mut candidate);
    if mode_reset_semantics_equal(store, &candidate, mode) && !plugin_instances_changed {
        return false;
    }

    *store = candidate;
    crate::state::native_element_id::rekey_mode_element_ids(store, mode);
    true
}

pub fn plan_custom_tab_delete(store: &AppStoreData, id: &str) -> Option<CustomTabDeletePlan> {
    store.custom_tabs.iter().position(|tab| tab.id == id)?;
    let mut tab_order = normalize_tab_order(&store.tab_order, &store.custom_tabs);
    let index = tab_order.iter().position(|tab_id| tab_id == id)?;
    let next_selected = if store.selected_key_type == id {
        if index > 0 {
            tab_order[index - 1].clone()
        } else {
            tab_order.get(1)?.clone()
        }
    } else {
        store.selected_key_type.clone()
    };
    tab_order.remove(index);
    let custom_tabs: Vec<CustomTab> = store
        .custom_tabs
        .iter()
        .filter(|tab| tab.id != id)
        .cloned()
        .collect();
    Some(CustomTabDeletePlan {
        custom_tabs,
        tab_order,
        next_selected,
    })
}

pub fn delete_custom_tab_data(store: &mut AppStoreData, id: &str, plan: &CustomTabDeletePlan) {
    store.custom_tabs = plan.custom_tabs.clone();
    store.tab_order = plan.tab_order.clone();
    store.bar_count = normalize_bar_count(store.bar_count, &store.tab_order);
    store.keys.remove(id);
    store.key_positions.remove(id);
    store.stat_positions.remove(id);
    store.graph_positions.remove(id);
    store.knob_positions.remove(id);
    store.sprite_positions.remove(id);
    store.layer_groups.remove(id);
    store.tab_css_overrides.remove(id);
    store.tab_note_overrides.remove(id);
    store.key_counters.remove(id);
    store.selected_key_type = plan.next_selected.clone();
}
