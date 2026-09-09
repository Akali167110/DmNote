use super::*;
const SOUND_EXTENSIONS: &[&str] = &["wav", "mp3", "ogg", "flac", "m4a", "aac", "aif", "aiff"];

fn sound_key(path: &str) -> Result<(), String> {
    if !is_web_asset_key(path) || !path.starts_with("/assets/sounds/") {
        return Err("invalid-web-sound-path".into());
    }
    Ok(())
}
fn listed_key(path: &str) -> bool {
    is_web_asset_key(path)
        && path.starts_with("/assets/sounds/")
        && !path.contains("/originals/")
        && SOUND_EXTENSIONS.contains(&extension(path, "").as_str())
}
pub(super) fn reconcile_library(store: &mut AppStoreData, assets: &WebAssetMap) {
    store
        .sound_library
        .retain(|key, _| assets.contains_key(key));
    for key in assets.keys().filter(|key| listed_key(key)) {
        store.sound_library.entry(key.clone()).or_default();
    }
}
pub fn list_sounds(
    store: &AppStoreData,
    assets: &WebAssetMap,
    modified_at: &BTreeMap<String, u64>,
) -> Result<Value, String> {
    let mut items = Vec::new();
    for (path, data) in assets.iter().filter(|(path, _)| listed_key(path)) {
        let bytes = BASE64_STANDARD
            .decode(data)
            .map_err(|e| format!("invalid-web-asset-data: {e}"))?;
        let meta = store.sound_library.get(path).cloned().unwrap_or_default();
        let mut item = json!({"soundPath":path,"fileName":Path::new(path).file_name().and_then(|name|name.to_str()).unwrap_or_default(),"sizeBytes":bytes.len(),"hidden":meta.hidden,"enabled":!meta.hidden,"source":meta.source});
        if let Some(value) = modified_at.get(path) {
            item["modifiedAtMs"] = json!(value);
        }
        if let Some(value) = meta.original_path {
            item["originalPath"] = json!(value);
        }
        if let Some(value) = meta.trim_start_ratio {
            item["trimStartRatio"] = json!(value);
        }
        if let Some(value) = meta.trim_end_ratio {
            item["trimEndRatio"] = json!(value);
        }
        if let Some(value) = meta.display_name {
            item["displayName"] = json!(value);
        }
        items.push(item);
    }
    items.sort_by(|a, b| {
        (b["source"] == "builtin")
            .cmp(&(a["source"] == "builtin"))
            .then_with(|| {
                b["modifiedAtMs"]
                    .as_u64()
                    .unwrap_or_default()
                    .cmp(&a["modifiedAtMs"].as_u64().unwrap_or_default())
            })
            .then_with(|| a["fileName"].as_str().cmp(&b["fileName"].as_str()))
    });
    Ok(Value::Array(items))
}
pub fn load_original(
    store: &AppStoreData,
    sound_path: &str,
    assets: &WebAssetMap,
) -> Result<Value, String> {
    sound_key(sound_path)?;
    let original = store
        .sound_library
        .get(sound_path)
        .and_then(|entry| entry.original_path.as_ref())
        .ok_or("원본 파일 정보가 없습니다.")?;
    let path = format!("/assets/sounds/{original}");
    sound_key(&path)?;
    let data = assets.get(&path).ok_or("원본 파일이 존재하지 않습니다.")?;
    let bytes = BASE64_STANDARD
        .decode(data)
        .map_err(|e| format!("원본 사운드 파일 읽기 실패: {e}"))?;
    Ok(
        json!({"success":true,"audioBase64":BASE64_STANDARD.encode(bytes),"originalExtension":extension(&path,"wav")}),
    )
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveWav {
    wav_base64: String,
    file_name: Option<String>,
    original_base64: Option<String>,
    original_extension: Option<String>,
    trim_start_ratio: Option<f64>,
    trim_end_ratio: Option<f64>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateWav {
    sound_path: String,
    wav_base64: String,
    trim_start_ratio: Option<f64>,
    trim_end_ratio: Option<f64>,
    display_name: Option<String>,
}
fn wav_bytes(encoded: &str) -> Result<Option<Vec<u8>>, String> {
    let bytes = BASE64_STANDARD
        .decode(encoded.trim())
        .map_err(|e| format!("사운드 데이터 디코딩 실패: {e}"))?;
    Ok(
        (bytes.len() >= 12 && bytes.get(..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WAVE"))
            .then_some(bytes),
    )
}
pub(super) fn prepare_sound(
    store: &mut AppStoreData,
    command: &str,
    args: &Value,
    memory: &WebPresetAssets,
    assets: &WebAssetMap,
    deletes: &mut Vec<String>,
) -> Result<Value, String> {
    match command {
        "sound_save_processed_wav" => {
            let request: SaveWav =
                serde_json::from_value(args.get("request").cloned().unwrap_or(Value::Null))
                    .map_err(|e| e.to_string())?;
            if request.wav_base64.trim().is_empty() {
                return Ok(json!({"success":false,"error":"사운드 데이터가 비어 있습니다."}));
            }
            let Some(bytes) = wav_bytes(&request.wav_base64)? else {
                return Ok(json!({"success":false,"error":"유효한 WAV 데이터가 아닙니다."}));
            };
            let path = new_path("sounds", "wav")?;
            memory.write(Path::new(&path), &bytes)?;
            let original_path = match request
                .original_base64
                .as_deref()
                .map(str::trim)
                .filter(|data| !data.is_empty())
            {
                Some(data) => {
                    let bytes = BASE64_STANDARD
                        .decode(data)
                        .map_err(|e| format!("원본 사운드 데이터 디코딩 실패: {e}"))?;
                    let path = new_path(
                        "sounds/originals",
                        &request
                            .original_extension
                            .as_deref()
                            .unwrap_or("wav")
                            .to_ascii_lowercase(),
                    )?;
                    memory.write(Path::new(&path), &bytes)?;
                    Some(path.strip_prefix("/assets/sounds/").unwrap().to_string())
                }
                None => None,
            };
            store.sound_library.insert(
                path.clone(),
                SoundLibraryEntry {
                    hidden: false,
                    source: SoundSource::Local,
                    original_path,
                    trim_start_ratio: request.trim_start_ratio,
                    trim_end_ratio: request.trim_end_ratio,
                    display_name: request.file_name,
                },
            );
            Ok(json!({"success":true,"soundPath":path}))
        }
        "sound_update_processed_wav" => {
            let request: UpdateWav =
                serde_json::from_value(args.get("request").cloned().unwrap_or(Value::Null))
                    .map_err(|e| e.to_string())?;
            sound_key(&request.sound_path)?;
            if store
                .sound_library
                .get(&request.sound_path)
                .is_some_and(|entry| entry.source == SoundSource::Builtin)
            {
                return Err("내장 사운드는 편집할 수 없습니다.".into());
            }
            let Some(bytes) = wav_bytes(&request.wav_base64)? else {
                return Ok(json!({"success":false,"error":"유효한 WAV 데이터가 아닙니다."}));
            };
            if !assets.contains_key(&request.sound_path) {
                return Err("편집할 사운드 파일을 찾을 수 없습니다.".into());
            }
            memory.write(Path::new(&request.sound_path), &bytes)?;
            if let Some(entry) = store.sound_library.get_mut(&request.sound_path) {
                entry.trim_start_ratio = request.trim_start_ratio;
                entry.trim_end_ratio = request.trim_end_ratio;
                if let Some(name) = request.display_name {
                    entry.display_name = Some(name);
                }
            }
            Ok(json!({"success":true}))
        }
        "sound_set_hidden" | "sound_set_enabled" => {
            let path = string(args, "soundPath")?;
            sound_key(path)?;
            if !assets.contains_key(path) {
                return Err("대상 사운드 파일이 존재하지 않습니다.".into());
            }
            let enabled_mode = command == "sound_set_enabled";
            let field = if enabled_mode { "enabled" } else { "hidden" };
            let value = args
                .get(field)
                .and_then(Value::as_bool)
                .ok_or_else(|| format!("missing-argument: {field}"))?;
            store.sound_library.entry(path.into()).or_default().hidden =
                if enabled_mode { !value } else { value };
            let mut result = json!({"success":true,"soundPath":path});
            result[field] = json!(value);
            Ok(result)
        }
        "sound_rename" => {
            let path = string(args, "soundPath")?;
            sound_key(path)?;
            if !assets.contains_key(path) {
                return Err("대상 사운드 파일이 존재하지 않습니다.".into());
            }
            let name = string(args, "displayName")?.trim();
            if name.is_empty() {
                return Err("사운드 이름이 비어 있습니다.".into());
            }
            let entry = store
                .sound_library
                .get_mut(path)
                .ok_or("대상 사운드가 존재하지 않습니다.")?;
            if entry.source == SoundSource::Builtin {
                return Err("내장 사운드는 이름을 변경할 수 없습니다.".into());
            }
            entry.display_name = Some(name.into());
            Ok(json!({"success":true,"displayName":name}))
        }
        "sound_delete" => {
            let path = string(args, "soundPath")?;
            sound_key(path)?;
            if store
                .sound_library
                .get(path)
                .is_some_and(|entry| entry.source == SoundSource::Builtin)
            {
                return Err("내장 사운드는 삭제할 수 없습니다.".into());
            }
            if assets.contains_key(path) {
                deletes.push(path.into());
            }
            if let Some(original) = store
                .sound_library
                .get(path)
                .and_then(|entry| entry.original_path.as_ref())
            {
                let original = format!("/assets/sounds/{original}");
                if sound_key(&original).is_ok() && assets.contains_key(&original) {
                    deletes.push(original);
                }
            }
            remove_sound_entry_and_references(store, path);
            Ok(json!({"success":true}))
        }
        _ => Err(format!("unsupported-web-resource-command: {command}")),
    }
}

pub fn remove_sound_entry_and_references(store: &mut AppStoreData, path_key: &str) -> bool {
    store.sound_library.remove(path_key);
    let mut references_changed = false;

    for positions in store.key_positions.values_mut() {
        for position in positions.iter_mut() {
            if position.sound_path.as_deref() == Some(path_key) {
                position.sound_path = None;
                position.sound_enabled = Some(false);
                references_changed = true;
            }
        }
    }

    for positions in store.stat_positions.values_mut() {
        for stat_position in positions.iter_mut() {
            if stat_position.position.sound_path.as_deref() == Some(path_key) {
                stat_position.position.sound_path = None;
                stat_position.position.sound_enabled = Some(false);
                references_changed = true;
            }
        }
    }

    for positions in store.graph_positions.values_mut() {
        for graph_position in positions.iter_mut() {
            if graph_position.position.sound_path.as_deref() == Some(path_key) {
                graph_position.position.sound_path = None;
                graph_position.position.sound_enabled = Some(false);
                references_changed = true;
            }
        }
    }

    for positions in store.knob_positions.values_mut() {
        for knob_position in positions.iter_mut() {
            if knob_position.position.sound_path.as_deref() == Some(path_key) {
                knob_position.position.sound_path = None;
                knob_position.position.sound_enabled = Some(false);
                references_changed = true;
            }
        }
    }

    references_changed
}
