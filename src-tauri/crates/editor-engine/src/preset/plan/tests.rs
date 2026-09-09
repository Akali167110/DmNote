use super::*;
use crate::preset::{export::PresetAssetReader, validation::decode_preset, PresetFile};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

struct NoAssets;
impl PresetAssetReader for NoAssets {
    fn read(&self, _path: &Path) -> Result<Vec<u8>, String> {
        Err("missing asset".into())
    }
    fn exists(&self, _path: &Path) -> bool {
        false
    }
    fn is_local(&self, path: &Path) -> bool {
        path.is_absolute()
    }
    fn image_source(&self, _reference: &str) -> Option<PathBuf> {
        None
    }
}
impl PresetImportHost for NoAssets {
    type Error = String;
    fn invalid_preset(message: String) -> Self::Error {
        message
    }
    fn normalize_custom_css(&self, _css: &mut CustomCss, _operation: &str) {}
    fn normalize_tab_css(&self, _css: &mut TabCss, _operation: &str) {}
    fn restore_preset_local_fonts(
        &self,
        _fonts: &mut FontSettings,
        _embedded: Option<&[EmbeddedLocalFont]>,
    ) -> Result<(), String> {
        Ok(())
    }
    fn restore_preset_local_images(
        &self,
        _keys: &mut KeyPositions,
        _stats: &mut StatPositions,
        _graphs: &mut GraphPositions,
        _knobs: &mut KnobPositions,
        _sprites: &mut SpritePositions,
        _embedded: Option<&[EmbeddedLocalImage]>,
    ) -> Result<(), String> {
        Ok(())
    }
    fn restore_preset_local_sounds(
        &self,
        _keys: &mut KeyPositions,
        _stats: &mut StatPositions,
        _graphs: &mut GraphPositions,
        _knobs: &mut KnobPositions,
        _embedded: Option<&[EmbeddedLocalSound]>,
    ) -> Result<(), String> {
        Ok(())
    }
}

fn store() -> AppStoreData {
    let mut data = AppStoreData {
        keys: HashMap::from([("4key".into(), vec!["A".into()])]),
        key_positions: HashMap::from([(
            "4key".into(),
            vec![KeyPosition {
                dx: 123.0,
                ..KeyPosition::default()
            }],
        )]),
        stat_positions: HashMap::new(),
        graph_positions: HashMap::new(),
        knob_positions: HashMap::new(),
        sprite_positions: HashMap::new(),
        layer_groups: HashMap::new(),
        selected_key_type: "4key".into(),
        ..AppStoreData::default()
    };
    crate::state::native_element_id::backfill_store_element_ids(&mut data);
    data
}
fn tab_preset() -> PresetFile {
    PresetFile {
        keys: Some(HashMap::from([("4key".into(), vec!["B".into()])])),
        ..PresetFile::default()
    }
}

#[test]
fn export_full_and_tab_preserve_distinct_optional_fields_and_exclude_private_storage() {
    let mut current = store();
    current.plugin_data.insert(
        "plugin_data_example/instances".into(),
        serde_json::json!([{"id":"private"}]),
    );
    let full =
        serde_json::to_value(build_full_preset(current.clone(), &NoAssets).unwrap()).unwrap();
    let tab = serde_json::to_value(build_tab_preset(current, &NoAssets).unwrap()).unwrap();
    assert!(full["backgroundColor"].is_string());
    assert!(tab["backgroundColor"].is_null());
    assert!(full["tabNoteOverrides"].is_object());
    assert!(tab["tabNoteOverrides"].is_object());
    assert!(full["tabCssOverrides"].is_object());
    assert!(tab["tabCssOverrides"].is_object());
    assert!(tab.get("tabOrder").is_none());
    assert!(tab.get("barCount").is_none());
    assert!(tab["customTabs"].is_null());
    assert!(full.get("pluginData").is_none());
    assert!(tab.get("pluginData").is_none());
    assert!(full["embeddedLocalImages"].is_null());
}

#[test]
fn full_export_decode_apply_roundtrip_rekeys_elements_and_keeps_layout() {
    let current = store();
    let original_id = current.key_positions["4key"][0].id.clone();
    let json =
        serde_json::to_string(&build_full_preset(current.clone(), &NoAssets).unwrap()).unwrap();
    let decoded = decode_preset(&json).unwrap();
    let plan = prepare_full_preset(decoded, &current, &NoAssets).unwrap();
    let mut target = store();
    target.key_positions.get_mut("4key").unwrap()[0].dx = 99.0;
    plan.apply(&mut target).unwrap();
    assert_eq!(target.keys, current.keys);
    assert_eq!(target.key_positions["4key"][0].dx, 123.0);
    assert_ne!(target.key_positions["4key"][0].id, original_id);
    assert_eq!(target.selected_key_type, "4key");
}

#[test]
fn full_import_preserves_absent_global_settings_and_css_override_field() {
    let mut current = store();
    current.background_color = "#123456".into();
    current.use_custom_js = true;
    current.tab_css_overrides.insert(
        "4key".into(),
        TabCss {
            path: None,
            content: ".key { opacity: .5 }".into(),
            enabled: true,
        },
    );
    current
        .tab_note_overrides
        .insert("4key".into(), TabNoteSettings::default());
    let expected_css = current.tab_css_overrides.clone();
    let plan = prepare_full_preset(tab_preset(), &current, &NoAssets).unwrap();
    assert_eq!(plan.publication.custom_js, current.custom_js);
    assert_eq!(plan.publication.custom_css, current.custom_css);
    assert!(plan.publication.use_custom_js);
    let mut target = current;
    plan.apply(&mut target).unwrap();
    assert_eq!(target.background_color, "#123456");
    assert!(target.use_custom_js);
    assert_eq!(target.tab_css_overrides, expected_css);
    assert!(target.tab_note_overrides.is_empty());
}

#[test]
fn tab_import_distinguishes_absent_collection_from_explicit_empty_collection() {
    let mut current = store();
    current.stat_positions.insert(
        "4key".into(),
        vec![StatPosition {
            position: KeyPosition::default(),
            stat_type: StatType::Kps,
        }],
    );
    crate::state::native_element_id::backfill_store_element_ids(&mut current);
    let original_key_id = current.key_positions["4key"][0].id.clone();
    let original_stats = current.stat_positions.clone();
    let plan = prepare_tab_preset(
        tab_preset(),
        "4key".into(),
        &current.font_settings,
        &NoAssets,
    )
    .unwrap();
    plan.apply(&mut current);
    assert_eq!(current.stat_positions, original_stats);
    assert_eq!(current.key_positions["4key"][0].id, original_key_id);
    assert_eq!(current.key_positions["4key"][0].dx, 123.0);
    let mut explicit_empty = tab_preset();
    explicit_empty.stat_positions = Some(HashMap::new());
    let plan = prepare_tab_preset(
        explicit_empty,
        "4key".into(),
        &current.font_settings,
        &NoAssets,
    )
    .unwrap();
    plan.apply(&mut current);
    assert!(current.stat_positions["4key"].is_empty());
}

#[test]
fn tab_import_rejects_ambiguous_source_with_existing_error_code() {
    let preset = PresetFile {
        keys: Some(HashMap::from([
            ("left".into(), vec![]),
            ("right".into(), vec![]),
        ])),
        ..PresetFile::default()
    };
    let error = prepare_tab_preset(preset, "4key".into(), &FontSettings::default(), &NoAssets)
        .err()
        .unwrap();
    assert_eq!(error, "tab-preset-ambiguous-source");
}

#[test]
fn tab_import_clears_explicit_empty_overrides_only_for_target_and_preserves_global_settings() {
    let mut current = store();
    current.background_color = "#123456".into();
    current.use_custom_js = true;
    for mode in ["4key", "5key"] {
        current
            .tab_css_overrides
            .insert(mode.into(), TabCss::default());
        current
            .tab_note_overrides
            .insert(mode.into(), TabNoteSettings::default());
    }
    let mut preset = tab_preset();
    preset.tab_css_overrides = Some(HashMap::new());
    preset.tab_note_overrides = Some(HashMap::new());
    preset.background_color = Some("#FFFFFF".into());
    preset.use_custom_js = Some(false);
    let plan =
        prepare_tab_preset(preset, "4key".into(), &current.font_settings, &NoAssets).unwrap();
    plan.apply(&mut current);
    assert!(!current.tab_css_overrides.contains_key("4key"));
    assert!(!current.tab_note_overrides.contains_key("4key"));
    assert!(current.tab_css_overrides.contains_key("5key"));
    assert!(current.tab_note_overrides.contains_key("5key"));
    assert_eq!(current.background_color, "#123456");
    assert!(current.use_custom_js);
}
