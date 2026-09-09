use super::super::{
    export::{
        build_preset_font_payload, build_preset_image_payload, build_preset_sound_payload,
        collect_used_font_families, PresetAssetReader,
    },
    PresetFile, BUILTIN_TAB_IDS,
};
use crate::models::*;
use std::collections::HashMap;
pub fn build_full_preset(
    snapshot: AppStoreData,
    assets: &impl PresetAssetReader,
) -> Result<PresetFile, String> {
    let used_font_families = collect_used_font_families(
        &snapshot.key_positions,
        &snapshot.stat_positions,
        &snapshot.graph_positions,
        &snapshot.knob_positions,
    );
    let (font_settings, embedded_local_fonts) =
        build_preset_font_payload(assets, &snapshot.font_settings, &used_font_families)?;
    let (
        key_positions,
        stat_positions,
        graph_positions,
        knob_positions,
        sprite_positions,
        embedded_local_images,
    ) = build_preset_image_payload(
        assets,
        &snapshot.key_positions,
        &snapshot.stat_positions,
        &snapshot.graph_positions,
        &snapshot.knob_positions,
        &snapshot.sprite_positions,
    )?;
    let (key_positions, stat_positions, graph_positions, knob_positions, embedded_local_sounds) =
        build_preset_sound_payload(
            assets,
            &key_positions,
            &stat_positions,
            &graph_positions,
            &knob_positions,
        )?;

    let preset = PresetFile {
        keys: Some(snapshot.keys),
        key_positions: Some(key_positions),
        stat_positions: Some(stat_positions),
        graph_positions: Some(graph_positions),
        knob_positions: Some(knob_positions),
        sprite_positions: Some(sprite_positions),
        background_color: Some(snapshot.background_color),
        note_settings: Some(snapshot.note_settings),
        note_effect: Some(snapshot.note_effect),
        laboratory_enabled: Some(snapshot.laboratory_enabled),
        custom_tabs: Some(snapshot.custom_tabs),
        tab_order: Some(snapshot.tab_order),
        bar_count: Some(snapshot.bar_count),
        selected_key_type: Some(snapshot.selected_key_type),
        use_custom_css: Some(snapshot.use_custom_css),
        custom_css: Some(snapshot.custom_css),
        use_custom_js: Some(snapshot.use_custom_js),
        custom_js: Some(snapshot.custom_js),
        font_settings: Some(font_settings),
        // 현재 형식은 빈 맵도 명시해 "필드 없음(과거 버전)"과 "비우기"를 구분
        tab_note_overrides: Some(snapshot.tab_note_overrides),
        layer_groups: Some(snapshot.layer_groups),
        tab_css_overrides: Some(snapshot.tab_css_overrides),
        embedded_local_fonts: (!embedded_local_fonts.is_empty()).then_some(embedded_local_fonts),
        embedded_local_images: (!embedded_local_images.is_empty()).then_some(embedded_local_images),
        embedded_local_sounds: (!embedded_local_sounds.is_empty()).then_some(embedded_local_sounds),
    };

    Ok(preset)
}

pub fn build_tab_preset(
    snapshot: AppStoreData,
    assets: &impl PresetAssetReader,
) -> Result<PresetFile, String> {
    let tab_id = snapshot.selected_key_type.clone();

    // 단일 탭 맵 구성 (embed helper용)
    let mut tab_key_positions: KeyPositions = HashMap::new();
    if let Some(positions) = snapshot.key_positions.get(&tab_id) {
        tab_key_positions.insert(tab_id.clone(), positions.clone());
    }
    let mut tab_stat_positions: StatPositions = HashMap::new();
    if let Some(positions) = snapshot.stat_positions.get(&tab_id) {
        tab_stat_positions.insert(tab_id.clone(), positions.clone());
    }
    let mut tab_graph_positions: GraphPositions = HashMap::new();
    if let Some(positions) = snapshot.graph_positions.get(&tab_id) {
        tab_graph_positions.insert(tab_id.clone(), positions.clone());
    }
    let mut tab_knob_positions: KnobPositions = HashMap::new();
    if let Some(positions) = snapshot.knob_positions.get(&tab_id) {
        tab_knob_positions.insert(tab_id.clone(), positions.clone());
    }
    let mut tab_sprite_positions: SpritePositions = HashMap::new();
    if let Some(positions) = snapshot.sprite_positions.get(&tab_id) {
        tab_sprite_positions.insert(tab_id.clone(), positions.clone());
    }

    let used_font_families = collect_used_font_families(
        &tab_key_positions,
        &tab_stat_positions,
        &tab_graph_positions,
        &tab_knob_positions,
    );
    let (font_settings, embedded_local_fonts) =
        build_preset_font_payload(assets, &snapshot.font_settings, &used_font_families)?;
    let (
        tab_key_positions,
        tab_stat_positions,
        tab_graph_positions,
        tab_knob_positions,
        tab_sprite_positions,
        embedded_local_images,
    ) = build_preset_image_payload(
        assets,
        &tab_key_positions,
        &tab_stat_positions,
        &tab_graph_positions,
        &tab_knob_positions,
        &tab_sprite_positions,
    )?;
    let (
        tab_key_positions,
        tab_stat_positions,
        tab_graph_positions,
        tab_knob_positions,
        embedded_local_sounds,
    ) = build_preset_sound_payload(
        assets,
        &tab_key_positions,
        &tab_stat_positions,
        &tab_graph_positions,
        &tab_knob_positions,
    )?;

    // 단일 탭 키 매핑
    let mut tab_keys: KeyMappings = HashMap::new();
    if let Some(keys) = snapshot.keys.get(&tab_id) {
        tab_keys.insert(tab_id.clone(), keys.clone());
    }

    // custom_tabs: 커스텀(비내장) 탭인 경우에만 포함
    let custom_tabs = if BUILTIN_TAB_IDS.contains(&tab_id.as_str()) {
        None
    } else {
        let found = snapshot
            .custom_tabs
            .iter()
            .find(|ct| ct.id == tab_id)
            .cloned();
        found.map(|ct| vec![ct])
    };

    // tab_note_overrides: 현재 탭 항목만 포함
    let tab_note_overrides = {
        let mut m: TabNoteOverrides = HashMap::new();
        if let Some(settings) = snapshot.tab_note_overrides.get(&tab_id) {
            m.insert(tab_id.clone(), settings.clone());
        }
        Some(m)
    };

    let mut tab_layer_groups = LayerGroups::new();
    tab_layer_groups.insert(
        tab_id.clone(),
        snapshot
            .layer_groups
            .get(&tab_id)
            .cloned()
            .unwrap_or_default(),
    );

    let mut tab_css_overrides = TabCssOverrides::new();
    if let Some(css) = snapshot.tab_css_overrides.get(&tab_id) {
        tab_css_overrides.insert(tab_id.clone(), css.clone());
    }

    let preset = PresetFile {
        keys: Some(tab_keys),
        key_positions: Some(tab_key_positions),
        stat_positions: Some(tab_stat_positions),
        graph_positions: Some(tab_graph_positions),
        knob_positions: Some(tab_knob_positions),
        sprite_positions: Some(tab_sprite_positions),
        background_color: None,
        note_settings: None,
        note_effect: None,
        laboratory_enabled: None,
        custom_tabs,
        tab_order: None,
        bar_count: None,
        selected_key_type: Some(tab_id),
        use_custom_css: None,
        custom_css: None,
        use_custom_js: None,
        custom_js: None,
        // 탭이 쓰는 폰트만 포함 — 로더가 현재 폰트 목록에 병합
        font_settings: Some(font_settings),
        tab_note_overrides,
        layer_groups: Some(tab_layer_groups),
        tab_css_overrides: Some(tab_css_overrides),
        embedded_local_fonts: (!embedded_local_fonts.is_empty()).then_some(embedded_local_fonts),
        embedded_local_images: (!embedded_local_images.is_empty()).then_some(embedded_local_images),
        embedded_local_sounds: (!embedded_local_sounds.is_empty()).then_some(embedded_local_sounds),
    };

    Ok(preset)
}
