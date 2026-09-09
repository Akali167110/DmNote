use super::*;
use crate::preset::validation::decode_preset;
use std::collections::HashMap;

fn store() -> AppStoreData {
    let mut store = AppStoreData {
        keys: HashMap::from([("4key".into(), vec!["A".into()])]),
        key_positions: HashMap::from([("4key".into(), vec![KeyPosition::default()])]),
        stat_positions: HashMap::new(),
        graph_positions: HashMap::new(),
        knob_positions: HashMap::new(),
        sprite_positions: HashMap::new(),
        selected_key_type: "4key".into(),
        ..AppStoreData::default()
    };
    crate::state::native_element_id::backfill_store_element_ids(&mut store);
    store
}
fn png() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend_from_slice(&48u32.to_be_bytes());
    bytes.extend_from_slice(&32u32.to_be_bytes());
    bytes
}
fn populated() -> (AppStoreData, WebAssetMap) {
    let mut store = store();
    let key = &mut store.key_positions.get_mut("4key").unwrap()[0];
    key.active_image = Some("/assets/images/image.png".into());
    key.inactive_image = key.active_image.clone();
    key.sound_path = Some("/assets/sounds/sound.wav".into());
    key.font_family = Some("Example".into());
    store.font_settings.custom_fonts.push(CustomFont {
        id: "font".into(),
        name: "Example".into(),
        display_name: "Example".into(),
        font_type: FontType::Local,
        local_path: Some("/assets/fonts/font.ttf".into()),
        enabled: true,
        css_content: None,
        weight_ranges: Vec::new(),
    });
    store.sprite_positions.insert(
        "4key".into(),
        vec![ReactiveSpritePosition {
            base_image: Some("/assets/images/image.png".into()),
            ..ReactiveSpritePosition::default()
        }],
    );
    let assets = BTreeMap::from([
        (
            "/assets/images/image.png".into(),
            BASE64_STANDARD.encode(png()),
        ),
        (
            "/assets/sounds/sound.wav".into(),
            BASE64_STANDARD.encode(b"sound-bytes"),
        ),
        (
            "/assets/fonts/font.ttf".into(),
            BASE64_STANDARD.encode(b"font-bytes"),
        ),
    ]);
    (store, assets)
}
#[test]
fn full_and_tab_roundtrip_embed_all_asset_types_without_blob_references() {
    for tab_only in [false, true] {
        let (source, assets) = populated();
        let exported = export_preset(&source, tab_only, &assets).unwrap();
        assert_eq!(exported.embedded_local_images.as_ref().unwrap().len(), 1);
        assert_eq!(exported.embedded_local_fonts.as_ref().unwrap().len(), 1);
        assert_eq!(exported.embedded_local_sounds.as_ref().unwrap().len(), 1);
        let decoded = decode_preset(&serde_json::to_string(&exported).unwrap()).unwrap();
        let target = store();
        let imported = prepare_import(&target, decoded, tab_only, &WebAssetMap::new()).unwrap();
        assert_eq!(imported.asset_writes.len(), 3);
        assert!(imported
            .asset_writes
            .keys()
            .all(|key| is_web_asset_key(key)));
        let key = &imported.store.key_positions["4key"][0];
        assert_eq!(key.active_image, key.inactive_image);
        assert_eq!(
            imported.store.sprite_positions["4key"][0]
                .reference_natural_size
                .as_ref()
                .unwrap()
                .width,
            48
        );
        let reexported = export_preset(&imported.store, tab_only, &imported.asset_writes).unwrap();
        assert_eq!(
            reexported.embedded_local_images.unwrap()[0].data_base64,
            BASE64_STANDARD.encode(png())
        );
        assert_eq!(
            reexported.embedded_local_fonts.unwrap()[0].data_base64,
            BASE64_STANDARD.encode(b"font-bytes")
        );
        assert_eq!(
            reexported.embedded_local_sounds.unwrap()[0].data_base64,
            BASE64_STANDARD.encode(b"sound-bytes")
        );
    }
}
#[test]
fn discarded_asset_preparation_does_not_mutate_existing_store_or_assets() {
    let (source, assets) = populated();
    let original_store = serde_json::to_value(&source).unwrap();
    let original_assets = assets.clone();
    let preset = export_preset(&source, false, &assets).unwrap();
    let prepared = prepare_import(&source, preset, false, &assets).unwrap();
    assert_eq!(prepared.asset_writes.len(), 3);
    assert!(prepared
        .asset_writes
        .keys()
        .all(|key| !assets.contains_key(key)));
    drop(prepared);
    assert_eq!(serde_json::to_value(&source).unwrap(), original_store);
    assert_eq!(assets, original_assets);
}
#[test]
fn missing_local_assets_fall_back_and_external_images_survive() {
    let (mut source, _) = populated();
    source.key_positions.get_mut("4key").unwrap()[0].inactive_image =
        Some("https://example.com/a.png".into());
    let preset =
        plan::build_full_preset(source, &WebPresetAssets::new(&WebAssetMap::new()).unwrap())
            .unwrap();
    let result = prepare_import(&store(), preset, false, &WebAssetMap::new()).unwrap();
    let key = &result.store.key_positions["4key"][0];
    assert!(key.active_image.is_none());
    assert!(key.sound_path.is_none());
    assert_eq!(
        key.inactive_image.as_deref(),
        Some("https://example.com/a.png")
    );
    assert!(!result.store.font_settings.custom_fonts[0].enabled);
    assert!(result.asset_writes.is_empty());
}
#[test]
fn malformed_asset_maps_and_failed_tab_import_leave_inputs_unchanged() {
    let target = store();
    let before = serde_json::to_value(&target).unwrap();
    let malformed = BTreeMap::from([("/assets/images/../secret".into(), "AA==".into())]);
    assert!(prepare_import(&target, PresetFile::default(), false, &malformed).is_err());
    let invalid = PresetFile {
        keys: Some(HashMap::from([("x".into(), vec![]), ("y".into(), vec![])])),
        ..PresetFile::default()
    };
    assert!(prepare_import(&target, invalid, true, &WebAssetMap::new()).is_err());
    assert_eq!(serde_json::to_value(&target).unwrap(), before);
}

#[test]
fn imported_css_keeps_content_but_drops_unusable_asset_paths() {
    use crate::web_resources::css::MAX_CSS_BYTES;
    let cases = [
        ("/assets/css/valid.CSS", b"body {}".to_vec(), true),
        ("/assets/css/invalid.txt", b"body {}".to_vec(), false),
        ("/assets/css/invalid.css", vec![0xff], false),
        (
            "/assets/css/large.css",
            vec![b' '; MAX_CSS_BYTES + 1],
            false,
        ),
        ("/assets/images/other.css", b"body {}".to_vec(), false),
    ];
    for (path, bytes, valid) in cases {
        let assets = WebAssetMap::from([(path.into(), BASE64_STANDARD.encode(bytes))]);
        let host = WebPresetAssets::new(&assets).unwrap();
        let mut global = CustomCss {
            path: Some(path.into()),
            content: "stored global".into(),
        };
        let mut tab = TabCss {
            path: Some(path.into()),
            content: "stored tab".into(),
            ..TabCss::default()
        };
        host.normalize_custom_css(&mut global, "test");
        host.normalize_tab_css(&mut tab, "test");
        assert_eq!(global.path.as_deref(), valid.then_some(path));
        assert_eq!(tab.path.as_deref(), valid.then_some(path));
        assert_eq!(global.content, "stored global");
        assert_eq!(tab.content, "stored tab");
        assert!(host.into_writes().is_empty());
    }
}
