use super::*;
fn wav(tag: u8) -> String {
    let mut bytes = b"RIFF\x04\0\0\0WAVE".to_vec();
    bytes.push(tag);
    BASE64_STANDARD.encode(bytes)
}
#[test]
fn processed_sound_original_update_and_discard_are_atomic_candidates() {
    let store = AppStoreData::default();
    let prepared = prepare_resources(&store,"sound_save_processed_wav",&json!({"request":{"wavBase64":wav(1),"fileName":"Edited","originalBase64":BASE64_STANDARD.encode(b"original"),"originalExtension":"mp3","trimStartRatio":0.2,"trimEndRatio":0.8}}),&[],&WebAssetMap::new()).unwrap();
    let path = prepared.result["soundPath"].as_str().unwrap();
    assert_eq!(prepared.asset_writes.len(), 2);
    let original = load_original(&prepared.store, path, &prepared.asset_writes).unwrap();
    assert_eq!(original["audioBase64"], BASE64_STANDARD.encode(b"original"));
    assert_eq!(original["originalExtension"], "mp3");
    let before = serde_json::to_value(&prepared.store).unwrap();
    let assets_before = prepared.asset_writes.clone();
    let update = prepare_resources(&prepared.store,"sound_update_processed_wav",&json!({"request":{"soundPath":path,"wavBase64":wav(2),"displayName":"Changed","trimStartRatio":0.1,"trimEndRatio":0.9}}),&[],&prepared.asset_writes).unwrap();
    assert_eq!(update.asset_writes[path], wav(2));
    assert_eq!(
        update.store.sound_library[path].display_name.as_deref(),
        Some("Changed")
    );
    drop(update);
    assert_eq!(serde_json::to_value(&prepared.store).unwrap(), before);
    assert_eq!(prepared.asset_writes, assets_before);
    assert!(store.sound_library.is_empty());
}
#[test]
fn sound_delete_stages_both_assets_and_clears_all_coupled_references() {
    let saved = prepare_resources(
        &AppStoreData::default(),
        "sound_save_processed_wav",
        &json!({"request":{"wavBase64":wav(1),"originalBase64":"AA=="}}),
        &[],
        &WebAssetMap::new(),
    )
    .unwrap();
    let path = saved.result["soundPath"].as_str().unwrap();
    let mut store = saved.store.clone();
    for position in store.key_positions.values_mut().flatten() {
        position.sound_path = Some(path.into());
        position.sound_enabled = Some(true);
    }
    let before = serde_json::to_value(&store).unwrap();
    let result = prepare_resources(
        &store,
        "sound_delete",
        &json!({"soundPath":path}),
        &[],
        &saved.asset_writes,
    )
    .unwrap();
    assert_eq!(result.asset_deletes.len(), 2);
    assert!(result.asset_writes.is_empty());
    assert!(!result.store.sound_library.contains_key(path));
    assert!(result
        .store
        .key_positions
        .values()
        .flatten()
        .all(|position| position.sound_path.is_none() && position.sound_enabled == Some(false)));
    assert_eq!(serde_json::to_value(&store).unwrap(), before);
    assert_eq!(saved.asset_writes.len(), 2);
}
#[test]
fn invalid_or_builtin_sound_edits_produce_no_asset_writes() {
    let mut store = AppStoreData::default();
    let path = "/assets/sounds/builtin.wav";
    store.sound_library.insert(
        path.into(),
        SoundLibraryEntry {
            source: SoundSource::Builtin,
            ..Default::default()
        },
    );
    let assets = WebAssetMap::from([(path.into(), wav(1))]);
    for command in ["sound_delete", "sound_rename", "sound_update_processed_wav"] {
        assert!(prepare_resources(&store,command,&json!({"soundPath":path,"displayName":"New","request":{"soundPath":path,"wavBase64":wav(2)}}),&[],&assets).is_err());
    }
    let result = prepare_resources(
        &store,
        "sound_save_processed_wav",
        &json!({"request":{"wavBase64":"AA=="}}),
        &[],
        &assets,
    )
    .unwrap();
    assert_eq!(result.result["success"], false);
    assert!(result.asset_writes.is_empty());
}
#[test]
fn resource_signature_validation_and_list_reconciliation_match_native_contracts() {
    let store = AppStoreData::default();
    let invalid = WebFile {
        name: "fake.png".into(),
        mime_type: "image/png".into(),
        data_base64: BASE64_STANDARD.encode(b"not image"),
    };
    let result = prepare_resources(
        &store,
        "image_load",
        &json!({}),
        &[invalid],
        &WebAssetMap::new(),
    )
    .unwrap();
    assert_eq!(result.result["errorCode"], "invalid-image-content");
    assert!(result.asset_writes.is_empty());
    let svg = WebFile {
        name: "art.svg".into(),
        mime_type: "image/svg+xml".into(),
        data_base64: BASE64_STANDARD.encode(b"<svg xmlns='http://www.w3.org/2000/svg'/>"),
    };
    let result = prepare_resources(
        &store,
        "image_load",
        &json!({}),
        &[svg],
        &WebAssetMap::new(),
    )
    .unwrap();
    assert_eq!(result.result["success"], true);
    assert_eq!(result.asset_writes.len(), 1);
    let assets = WebAssetMap::from([
        ("/assets/sounds/new.wav".into(), wav(1)),
        ("/assets/sounds/originals/source.mp3".into(), "AA==".into()),
    ]);
    let result = prepare_resources(
        &store,
        "sound_list",
        &json!({"assetModifiedAt":{"/assets/sounds/new.wav":123}}),
        &[],
        &assets,
    )
    .unwrap();
    assert!(result
        .store
        .sound_library
        .contains_key("/assets/sounds/new.wav"));
    assert_eq!(result.result.as_array().unwrap().len(), 1);
    assert_eq!(result.result[0]["enabled"], true);
    assert_eq!(result.result[0]["modifiedAtMs"], 123);
}
