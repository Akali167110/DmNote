use super::super::{
    assets::{
        merge_prepared_tab_preset_fonts, migrate_imported_font_weights, prepare_tab_preset_fonts,
    },
    import::*,
    PresetFile,
};
use super::{FullPresetPublication, ImportedCssPaths, PresetImportHost};
use crate::settings::apply_patch_to_store;
use crate::{
    defaults::{default_keys, default_positions},
    errors::EditorCommitError,
    models::*,
    state::{
        editor::validate_history_restore_metadata,
        tab_metadata::{
            legacy_tab_order, normalize_bar_count, normalize_tab_order,
            reconcile_custom_tab_metadata,
        },
    },
};
use std::collections::HashMap;
fn normalize_tab_css_overrides(
    host: &impl PresetImportHost,
    mut overrides: TabCssOverrides,
    operation: &str,
) -> TabCssOverrides {
    for css in overrides.values_mut() {
        host.normalize_tab_css(css, operation);
    }
    overrides
}
pub struct FullPresetPlan {
    pub imported_css_paths: ImportedCssPaths,
    pub publication: FullPresetPublication,
    keys: KeyMappings,
    positions: KeyPositions,
    stat_positions: StatPositions,
    graph_positions: GraphPositions,
    knob_positions: KnobPositions,
    sprite_positions: SpritePositions,
    custom_tabs: Vec<CustomTab>,
    tab_order: Vec<String>,
    bar_count: u8,
    requested_selected_key_type: Option<String>,
    tab_note_overrides: TabNoteOverrides,
    preset_layer_groups: LayerGroups,
    preset_tab_css_overrides: Option<TabCssOverrides>,
    settings_patch: SettingsPatchInput,
}

pub fn prepare_full_preset<H: PresetImportHost>(
    mut preset: PresetFile,
    current: &AppStoreData,
    host: &H,
) -> Result<FullPresetPlan, H::Error> {
    let has_imported_global_css = preset.custom_css.is_some();
    let resolved_settings = resolve_full_preset_settings(&mut preset, current);

    let mut keys = preset.keys.unwrap_or_else(|| default_keys().clone());
    normalize_key_mappings(&mut keys);
    let mut positions = preset
        .key_positions
        .unwrap_or_else(|| default_positions().clone());
    let mut stat_positions = preset.stat_positions.unwrap_or_default();
    let mut graph_positions = preset.graph_positions.unwrap_or_default();
    let mut knob_positions = preset.knob_positions.unwrap_or_default();
    let mut sprite_positions = preset.sprite_positions.unwrap_or_default();
    migrate_imported_font_weights(
        &mut positions,
        &mut stat_positions,
        &mut graph_positions,
        &mut knob_positions,
    );
    let mut custom_tabs = preset
        .custom_tabs
        .take()
        .unwrap_or_else(|| synthesize_custom_tabs(&keys));
    let requested_selected_key_type = preset.selected_key_type;
    let preset_layer_groups = resolve_full_preset_layer_groups(preset.layer_groups, &keys);
    let preset_tab_css_overrides = preset
        .tab_css_overrides
        .map(|overrides| normalize_tab_css_overrides(host, overrides, "preset_load"));

    let mut desired_settings = resolved_settings.note_settings;
    desired_settings.migrate_fade_position();
    let note_patch = NoteSettingsPatch {
        frame_limit: Some(desired_settings.frame_limit),
        speed: Some(desired_settings.speed),
        track_height: Some(desired_settings.track_height),
        reverse: Some(desired_settings.reverse),
        fade_position: Some(desired_settings.fade_position),
        fade_top_px: Some(desired_settings.fade_top_px),
        fade_bottom_px: Some(desired_settings.fade_bottom_px),
        reverse_fade_top_px: Some(desired_settings.reverse_fade_top_px),
        reverse_fade_bottom_px: Some(desired_settings.reverse_fade_bottom_px),
        delayed_note_enabled: Some(desired_settings.delayed_note_enabled),
        short_note_threshold_ms: Some(desired_settings.short_note_threshold_ms),
        short_note_min_length_px: Some(desired_settings.short_note_min_length_px),
        key_display_delay_ms: Some(desired_settings.key_display_delay_ms),
    };

    let css_use = resolved_settings.use_custom_css;
    let mut custom_css = resolved_settings.custom_css;
    host.normalize_custom_css(&mut custom_css, "preset_load");
    let imported_css_paths = ImportedCssPaths {
        global: if has_imported_global_css {
            custom_css.path.clone()
        } else {
            None
        },
        tabs: preset_tab_css_overrides
            .as_ref()
            .into_iter()
            .flat_map(|overrides| overrides.values())
            .filter_map(|css| css.path.clone())
            .collect(),
    };
    let js_use = resolved_settings.use_custom_js;
    let custom_js = resolved_settings.custom_js;
    let has_font_settings = preset.font_settings.is_some();
    let mut preset_font_settings = preset.font_settings.clone().unwrap_or_default();
    if has_font_settings {
        host.restore_preset_local_fonts(
            &mut preset_font_settings,
            preset.embedded_local_fonts.as_deref(),
        )?;
    }
    host.restore_preset_local_images(
        &mut positions,
        &mut stat_positions,
        &mut graph_positions,
        &mut knob_positions,
        &mut sprite_positions,
        preset.embedded_local_images.as_deref(),
    )?;
    host.restore_preset_local_sounds(
        &mut positions,
        &mut stat_positions,
        &mut graph_positions,
        &mut knob_positions,
        preset.embedded_local_sounds.as_deref(),
    )?;
    align_imported_key_collections(&mut keys, &mut positions);
    // 이름 길이·예약어로는 거절하지 않는다. 예전 앱이나 다른 사람이 만든 프리셋이
    // 통째로 안 열린다 - 손상 복구를 항목 단위로 하는 이 저장소 방침과 어긋난다.
    // 구조가 어긋난 것도 id로 결합이 확정되면 여기서 메운다. 아래 검증에 남는 것은
    // 대응을 추측해야 하는 손상뿐이다
    reconcile_custom_tab_metadata(&mut custom_tabs, &mut keys, &mut positions);
    // 정렬은 화해가 끝난 목록을 봐야 한다. 고아 모드에서 살아난 탭도 순서에 들어간다
    let tab_order = preset
        .tab_order
        .take()
        .map(|order| normalize_tab_order(&order, &custom_tabs))
        .unwrap_or_else(|| legacy_tab_order(&custom_tabs));
    let bar_count = normalize_bar_count(
        preset.bar_count.unwrap_or_else(default_bar_count),
        &tab_order,
    );

    // 탭별 노트 설정 복원 (없으면 빈 맵으로 초기화 → 전역 폴백)
    let mut tab_note_overrides = preset.tab_note_overrides.unwrap_or_default();
    for tab in tab_note_overrides.values_mut() {
        tab.migrate_fade_position();
    }

    let settings_patch = SettingsPatchInput {
        background_color: Some(resolved_settings.background_color),
        note_settings: Some(note_patch),
        note_effect: Some(resolved_settings.note_effect),
        laboratory_enabled: Some(resolved_settings.laboratory_enabled),
        use_custom_css: Some(css_use),
        custom_css: Some(CustomCssPatch {
            path: Some(custom_css.path.clone()),
            content: Some(custom_css.content.clone()),
        }),
        font_settings: has_font_settings.then_some(preset_font_settings),
        use_custom_js: Some(js_use),
        custom_js: Some(CustomJsPatch {
            path: Some(custom_js.path.clone()),
            content: Some(custom_js.content.clone()),
            plugins: Some(custom_js.plugins.clone()),
        }),
        ..SettingsPatchInput::default()
    };
    Ok(FullPresetPlan {
        imported_css_paths,
        publication: FullPresetPublication {
            use_custom_css: css_use,
            custom_css,
            use_custom_js: js_use,
            custom_js,
        },
        keys,
        positions,
        stat_positions,
        graph_positions,
        knob_positions,
        sprite_positions,
        custom_tabs,
        tab_order,
        bar_count,
        requested_selected_key_type,
        tab_note_overrides,
        preset_layer_groups,
        preset_tab_css_overrides,
        settings_patch,
    })
}

pub type FullPresetApplyResult = (
    SettingsDiff,
    TabCssOverrides,
    Vec<CustomTab>,
    Vec<String>,
    u8,
    TabNoteOverrides,
    TabCssOverrides,
);
impl FullPresetPlan {
    /// 호스트의 변경 준비 구간에서 적용. 실패 시 scratch를 폐기한다.
    pub fn apply(
        self,
        store: &mut AppStoreData,
    ) -> Result<FullPresetApplyResult, EditorCommitError> {
        let Self {
            keys,
            positions,
            stat_positions,
            graph_positions,
            knob_positions,
            sprite_positions,
            custom_tabs,
            tab_order,
            bar_count,
            requested_selected_key_type,
            tab_note_overrides,
            preset_layer_groups,
            preset_tab_css_overrides,
            settings_patch,
            ..
        } = self;
        let previous_tab_css_overrides = store.tab_css_overrides.clone();
        let selected_key_type = choose_selected_key_type(
            requested_selected_key_type,
            &keys,
            store.selected_key_type.clone(),
        );
        store.keys = keys;
        store.key_positions = positions;
        store.stat_positions = stat_positions;
        store.graph_positions = graph_positions;
        store.knob_positions = knob_positions;
        store.sprite_positions = sprite_positions;
        store.custom_tabs = custom_tabs;
        store.tab_order = tab_order;
        store.bar_count = bar_count;
        store.selected_key_type = selected_key_type;
        validate_history_restore_metadata(
            &crate::models::EditorDocumentV1::from_store(store),
            &store.custom_tabs,
            &store.tab_order,
            &store.selected_key_type,
        )?;
        store.tab_note_overrides = tab_note_overrides;
        store.layer_groups = preset_layer_groups;
        if let Some(tab_css_overrides) = preset_tab_css_overrides {
            store.tab_css_overrides = tab_css_overrides;
        }
        rekey_full_preset_elements(store);
        crate::state::migration::clear_dangling_group_ids(store);
        let diff = apply_patch_to_store(store, &settings_patch);
        Ok((
            diff,
            previous_tab_css_overrides,
            store.custom_tabs.clone(),
            store.tab_order.clone(),
            store.bar_count,
            store.tab_note_overrides.clone(),
            store.tab_css_overrides.clone(),
        ))
    }
}

pub struct TabPresetPlan {
    pub imported_css_paths: ImportedCssPaths,
    current_tab_id: String,
    src_keys: Vec<KeySlot>,
    imported_key_positions: Option<Vec<KeyPosition>>,
    imported_stat_positions: Option<Vec<StatPosition>>,
    imported_graph_positions: Option<Vec<GraphPosition>>,
    imported_knob_positions: Option<Vec<KnobPosition>>,
    imported_sprite_positions: Option<Vec<ReactiveSpritePosition>>,
    has_tab_note_overrides: bool,
    imported_override: Option<TabNoteSettings>,
    imported_groups: Option<Vec<LayerGroupDef>>,
    imported_tab_css: Option<Option<TabCss>>,
    prepared_font_settings: Option<FontSettings>,
}

pub fn prepare_tab_preset<H: PresetImportHost>(
    preset: PresetFile,
    current_tab_id: String,
    existing_font_settings: &FontSettings,
    host: &H,
) -> Result<TabPresetPlan, H::Error> {
    let PresetFile {
        keys,
        key_positions,
        stat_positions,
        graph_positions,
        knob_positions,
        sprite_positions,
        selected_key_type,
        tab_note_overrides,
        layer_groups,
        tab_css_overrides,
        font_settings,
        embedded_local_fonts,
        embedded_local_images,
        embedded_local_sounds,
        ..
    } = preset;

    let mut imported_keys = keys.unwrap_or_default();
    normalize_key_mappings(&mut imported_keys);
    let source_tab_id = choose_tab_preset_source_tab(
        &imported_keys,
        selected_key_type.as_deref(),
        &current_tab_id,
    )
    .map_err(H::invalid_preset)?;
    let src_keys = imported_keys
        .get(&source_tab_id)
        .cloned()
        .ok_or_else(|| H::invalid_preset("invalid-tab-preset".to_string()))?;

    let imported_key_positions = key_positions.unwrap_or_default();
    let mut src_key_positions: KeyPositions = HashMap::new();
    if let Some(v) = imported_key_positions.get(&source_tab_id) {
        src_key_positions.insert(current_tab_id.clone(), v.clone());
    }

    let mut src_stat_positions =
        select_tab_preset_elements(stat_positions.as_ref(), &source_tab_id, &current_tab_id);
    let mut src_graph_positions =
        select_tab_preset_elements(graph_positions.as_ref(), &source_tab_id, &current_tab_id);
    let mut src_knob_positions =
        select_tab_preset_elements(knob_positions.as_ref(), &source_tab_id, &current_tab_id);
    let mut src_sprite_positions =
        select_tab_preset_elements(sprite_positions.as_ref(), &source_tab_id, &current_tab_id);

    migrate_imported_font_weights(
        &mut src_key_positions,
        &mut src_stat_positions,
        &mut src_graph_positions,
        &mut src_knob_positions,
    );

    let has_tab_note_overrides = tab_note_overrides.is_some();
    let mut imported_tab_note_overrides = tab_note_overrides.unwrap_or_default();
    for tab in imported_tab_note_overrides.values_mut() {
        tab.migrate_fade_position();
    }

    // 내장 에셋 복원
    host.restore_preset_local_images(
        &mut src_key_positions,
        &mut src_stat_positions,
        &mut src_graph_positions,
        &mut src_knob_positions,
        &mut src_sprite_positions,
        embedded_local_images.as_deref(),
    )?;
    host.restore_preset_local_sounds(
        &mut src_key_positions,
        &mut src_stat_positions,
        &mut src_graph_positions,
        &mut src_knob_positions,
        embedded_local_sounds.as_deref(),
    )?;

    let imported_key_positions = src_key_positions.remove(&current_tab_id);
    let imported_stat_positions = src_stat_positions.remove(&current_tab_id);
    let imported_graph_positions = src_graph_positions.remove(&current_tab_id);
    let imported_knob_positions = src_knob_positions.remove(&current_tab_id);
    let imported_sprite_positions = src_sprite_positions.remove(&current_tab_id);
    let imported_override = imported_tab_note_overrides.get(&source_tab_id).cloned();
    let imported_groups =
        layer_groups.map(|groups| groups.get(&source_tab_id).cloned().unwrap_or_default());
    let imported_tab_css = tab_css_overrides.map(|overrides| {
        overrides.get(&source_tab_id).cloned().map(|mut css| {
            host.normalize_tab_css(&mut css, "preset_load_tab");
            css
        })
    });
    let imported_css_paths = ImportedCssPaths {
        global: None,
        tabs: imported_tab_css
            .as_ref()
            .and_then(|css| css.as_ref())
            .and_then(|css| css.path.clone())
            .into_iter()
            .collect(),
    };

    // 프리셋에 담긴 폰트를 현재 폰트 목록에 병합 (탭 로드는 전역 설정을 덮지 않음)
    let prepared_font_settings = if let Some(imported_fonts) = font_settings {
        prepare_tab_preset_fonts(existing_font_settings, imported_fonts, |filtered_fonts| {
            host.restore_preset_local_fonts(filtered_fonts, embedded_local_fonts.as_deref())
        })?
    } else {
        None
    };

    Ok(TabPresetPlan {
        imported_css_paths,
        current_tab_id,
        src_keys,
        imported_key_positions,
        imported_stat_positions,
        imported_graph_positions,
        imported_knob_positions,
        imported_sprite_positions,
        has_tab_note_overrides,
        imported_override,
        imported_groups,
        imported_tab_css,
        prepared_font_settings,
    })
}

pub type TabPresetApplyResult = (
    Option<SettingsDiff>,
    TabCssOverrides,
    TabNoteOverrides,
    TabCssOverrides,
);
impl TabPresetPlan {
    pub fn apply(self, store: &mut AppStoreData) -> TabPresetApplyResult {
        let Self {
            current_tab_id,
            src_keys,
            imported_key_positions,
            imported_stat_positions,
            imported_graph_positions,
            imported_knob_positions,
            imported_sprite_positions,
            has_tab_note_overrides,
            imported_override,
            imported_groups,
            imported_tab_css,
            prepared_font_settings,
            ..
        } = self;
        let previous_tab_css_overrides = store.tab_css_overrides.clone();
        let key_positions_written = imported_key_positions.is_some();
        let stat_positions_written = imported_stat_positions.is_some();
        let graph_positions_written = imported_graph_positions.is_some();
        let knob_positions_written = imported_knob_positions.is_some();
        let sprite_positions_written = imported_sprite_positions.is_some();
        merge_tab_preset_key_pair(store, &current_tab_id, src_keys, imported_key_positions);
        if let Some(positions) = imported_stat_positions {
            store
                .stat_positions
                .insert(current_tab_id.clone(), positions);
        }
        if let Some(positions) = imported_graph_positions {
            store
                .graph_positions
                .insert(current_tab_id.clone(), positions);
        }
        if let Some(positions) = imported_knob_positions {
            store
                .knob_positions
                .insert(current_tab_id.clone(), positions);
        }
        if let Some(positions) = imported_sprite_positions {
            store
                .sprite_positions
                .insert(current_tab_id.clone(), positions);
        }
        rekey_tab_preset_elements(
            store,
            &current_tab_id,
            key_positions_written,
            stat_positions_written,
            graph_positions_written,
            knob_positions_written,
            sprite_positions_written,
        );
        apply_tab_note_override(
            store,
            &current_tab_id,
            has_tab_note_overrides,
            imported_override,
        );
        if let Some(groups) = imported_groups {
            store.layer_groups.insert(current_tab_id.clone(), groups);
        }
        if let Some(css) = imported_tab_css {
            if let Some(css) = css {
                store.tab_css_overrides.insert(current_tab_id.clone(), css);
            } else {
                store.tab_css_overrides.remove(&current_tab_id);
            }
        }

        let settings_diff = prepared_font_settings
            .and_then(|prepared| merge_prepared_tab_preset_fonts(&store.font_settings, prepared))
            .map(|font_settings| {
                apply_patch_to_store(
                    store,
                    &SettingsPatchInput {
                        font_settings: Some(font_settings),
                        ..SettingsPatchInput::default()
                    },
                )
            });
        crate::state::migration::clear_dangling_group_ids(store);
        (
            settings_diff,
            previous_tab_css_overrides,
            store.tab_note_overrides.clone(),
            store.tab_css_overrides.clone(),
        )
    }
}
