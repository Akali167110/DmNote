use super::PresetFile;
use crate::{defaults::default_keys, models::*};
use std::collections::{BTreeSet, HashMap};
pub struct ResolvedFullPresetSettings {
    pub background_color: String,
    pub note_settings: NoteSettings,
    pub note_effect: bool,
    pub laboratory_enabled: bool,
    pub use_custom_css: bool,
    pub custom_css: CustomCss,
    pub use_custom_js: bool,
    pub custom_js: CustomJs,
}

/// 과거 프리셋에 없던 전역 필드는 현재 값을 보존해 신규 설정을 지우지 않음
pub fn resolve_full_preset_settings(
    preset: &mut PresetFile,
    current: &AppStoreData,
) -> ResolvedFullPresetSettings {
    ResolvedFullPresetSettings {
        background_color: preset
            .background_color
            .take()
            .unwrap_or_else(|| current.background_color.clone()),
        note_settings: preset
            .note_settings
            .take()
            .unwrap_or_else(|| current.note_settings.clone()),
        note_effect: preset.note_effect.unwrap_or(current.note_effect),
        laboratory_enabled: preset
            .laboratory_enabled
            .unwrap_or(current.laboratory_enabled),
        use_custom_css: preset.use_custom_css.unwrap_or(current.use_custom_css),
        custom_css: preset
            .custom_css
            .take()
            .unwrap_or_else(|| current.custom_css.clone()),
        use_custom_js: preset.use_custom_js.unwrap_or(current.use_custom_js),
        custom_js: preset
            .custom_js
            .take()
            .unwrap_or_else(|| current.custom_js.clone()),
    }
}

pub fn select_tab_preset_elements<T: Clone>(
    collection: Option<&HashMap<String, Vec<T>>>,
    source_tab_id: &str,
    current_tab_id: &str,
) -> HashMap<String, Vec<T>> {
    let mut selected = HashMap::new();
    if let Some(collection) = collection {
        selected.insert(
            current_tab_id.to_string(),
            collection.get(source_tab_id).cloned().unwrap_or_default(),
        );
    }
    selected
}

pub fn choose_tab_preset_source_tab(
    keys: &KeyMappings,
    selected_key_type: Option<&str>,
    current_tab_id: &str,
) -> Result<String, String> {
    if keys.is_empty() {
        return Err("invalid-tab-preset".to_string());
    }

    if keys.contains_key(current_tab_id) {
        return Ok(current_tab_id.to_string());
    }

    if let Some(selected) = selected_key_type {
        if keys.contains_key(selected) {
            return Ok(selected.to_string());
        }
    }

    if keys.len() == 1 {
        if let Some(only) = keys.keys().next() {
            return Ok(only.clone());
        }
    }

    Err("tab-preset-ambiguous-source".to_string())
}

pub fn align_imported_key_pair(keys: &mut Vec<KeySlot>, positions: &mut Vec<KeyPosition>) {
    if keys.len() < positions.len() {
        keys.resize(positions.len(), KeySlot::default());
    } else if positions.len() < keys.len() {
        positions.resize(keys.len(), KeyPosition::default());
    }
}

pub fn align_imported_key_collections(keys: &mut KeyMappings, positions: &mut KeyPositions) {
    let modes = keys
        .keys()
        .chain(positions.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for mode in modes {
        let mode_keys = keys.entry(mode.clone()).or_default();
        let mode_positions = positions.entry(mode).or_default();
        align_imported_key_pair(mode_keys, mode_positions);
    }
}

pub fn rekey_full_preset_elements(store: &mut AppStoreData) {
    crate::state::native_element_id::rekey_store_element_ids(store);
}

pub fn rekey_tab_preset_elements(
    store: &mut AppStoreData,
    tab_id: &str,
    key_positions_written: bool,
    stat_positions_written: bool,
    graph_positions_written: bool,
    knob_positions_written: bool,
    sprite_positions_written: bool,
) {
    crate::state::native_element_id::rekey_mode_element_ids_for_collections(
        store,
        tab_id,
        key_positions_written,
        stat_positions_written,
        graph_positions_written,
        knob_positions_written,
        sprite_positions_written,
    );
    // 프리셋이 위치를 주지 않은 컬렉션은 기존 요소가 값 그대로 남는다 -
    // 신원을 회전시키지 않고 정렬이 덧붙인 빈 항목만 채운다
    crate::state::native_element_id::backfill_mode_element_ids_for_collections(
        store,
        tab_id,
        !key_positions_written,
        !stat_positions_written,
        !graph_positions_written,
        !knob_positions_written,
        !sprite_positions_written,
    );
}

pub fn merge_tab_preset_key_pair(
    store: &mut AppStoreData,
    current_tab_id: &str,
    mut keys: Vec<KeySlot>,
    imported_positions: Option<Vec<KeyPosition>>,
) {
    let mut positions = imported_positions.unwrap_or_else(|| {
        store
            .key_positions
            .get(current_tab_id)
            .cloned()
            .unwrap_or_default()
    });
    align_imported_key_pair(&mut keys, &mut positions);
    store.keys.insert(current_tab_id.to_string(), keys);
    store
        .key_positions
        .insert(current_tab_id.to_string(), positions);
}

pub fn synthesize_custom_tabs(keys: &KeyMappings) -> Vec<crate::models::CustomTab> {
    let default_modes = default_keys();
    let mut custom_ids = keys
        .keys()
        .filter(|key| !default_modes.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    custom_ids.sort();
    custom_ids
        .into_iter()
        .enumerate()
        .map(|(index, id)| crate::models::CustomTab {
            id,
            name: format!("Custom {}", index + 1),
        })
        .collect()
}

pub fn choose_selected_key_type(
    requested: Option<String>,
    keys: &KeyMappings,
    fallback: String,
) -> String {
    if let Some(req) = requested {
        if keys.contains_key(&req) {
            return req;
        }
    }
    if keys.contains_key(&fallback) {
        return fallback;
    }
    "4key".to_string()
}

pub fn apply_tab_note_override(
    store: &mut AppStoreData,
    tab_id: &str,
    field_present: bool,
    imported: Option<TabNoteSettings>,
) {
    if !field_present {
        return;
    }
    if let Some(settings) = imported {
        store
            .tab_note_overrides
            .insert(tab_id.to_string(), settings);
    } else {
        store.tab_note_overrides.remove(tab_id);
    }
}

pub fn resolve_full_preset_layer_groups(
    imported: Option<LayerGroups>,
    keys: &KeyMappings,
) -> LayerGroups {
    imported.unwrap_or_else(|| {
        keys.keys()
            .cloned()
            .map(|mode| (mode, Vec::new()))
            .collect()
    })
}
