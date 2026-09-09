//! 파일 선택은 브라우저가 수행하며 바이트와 메타데이터는 하나의 후보로 준비한다.
pub mod counter_animation;
pub(crate) mod css;
pub mod font;
pub mod image;
mod sound;
use crate::preset::assets::PresetAssetWriter;
use crate::{
    models::*,
    web_preset::{is_web_asset_key, WebAssetMap, WebPresetAssets},
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebFile {
    pub name: String,
    #[serde(default)]
    pub mime_type: String,
    pub data_base64: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebResourcePreparation {
    pub store: AppStoreData,
    pub result: Value,
    pub asset_writes: WebAssetMap,
    pub asset_deletes: Vec<String>,
}
pub use sound::{list_sounds, load_original, remove_sound_entry_and_references};

pub fn prepare_resources(
    store: &AppStoreData,
    command: &str,
    args: &Value,
    files: &[WebFile],
    assets: &WebAssetMap,
) -> Result<WebResourcePreparation, String> {
    let memory = WebPresetAssets::new(assets)?;
    let mut candidate = store.clone();
    let mut deletes = Vec::new();
    let result = match command {
        "image_load" | "font_load" | "sound_load" => match files.first() {
            None => json!({"success": false}),
            Some(file) => load_file(&mut candidate, command, file, &memory)?,
        },
        "sound_list" => {
            sound::reconcile_library(&mut candidate, assets);
            let modified =
                serde_json::from_value(args.get("assetModifiedAt").cloned().unwrap_or(json!({})))
                    .map_err(|e| format!("invalid-asset-modified-at: {e}"))?;
            list_sounds(&candidate, assets, &modified)?
        }
        "sound_load_original" => load_original(&candidate, string(args, "soundPath")?, assets)?,
        _ => sound::prepare_sound(&mut candidate, command, args, &memory, assets, &mut deletes)?,
    };
    Ok(WebResourcePreparation {
        store: candidate,
        result,
        asset_writes: memory.into_writes(),
        asset_deletes: deletes,
    })
}
fn string<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing-argument: {name}"))
}
fn extension(name: &str, fallback: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or(fallback)
        .to_ascii_lowercase()
}
fn new_path(category: &str, extension: &str) -> Result<String, String> {
    let path = format!("/assets/{category}/{}.{}", uuid::Uuid::new_v4(), extension);
    if !is_web_asset_key(&path) {
        return Err("invalid-web-asset-extension".into());
    }
    Ok(path)
}
fn load_file(
    store: &mut AppStoreData,
    command: &str,
    file: &WebFile,
    assets: &WebPresetAssets,
) -> Result<Value, String> {
    let bytes = BASE64_STANDARD
        .decode(&file.data_base64)
        .map_err(|e| format!("invalid-file-data: {e}"))?;
    match command {
        "image_load" => {
            if !image::image_bytes_have_supported_content(&bytes) {
                return Ok(
                    json!({"success":false,"error":"Selected file is not valid image content","errorCode":"invalid-image-content"}),
                );
            }
            let ext = image::normalize_image_extension(
                Path::new(&file.name).extension().and_then(|e| e.to_str()),
            );
            let path = new_path("images", &ext)?;
            assets.write(Path::new(&path), &bytes)?;
            Ok(json!({"success":true,"imagePath":path}))
        }
        "font_load" => {
            if !font::font_bytes_have_supported_content(&bytes) {
                return Ok(
                    json!({"success":false,"error":"Selected file is not valid font content","errorCode":"invalid-font-content"}),
                );
            }
            let metadata = match font::parse_font_metadata_bytes(&bytes) {
                Ok(metadata) => metadata,
                Err(error) => {
                    return Ok(
                        json!({"success":false,"error":format!("폰트 정보를 읽을 수 없습니다: {error}"),"errorCode":"invalid-font-content"}),
                    )
                }
            };
            let name = metadata.family_name.unwrap_or_else(|| {
                Path::new(&file.name)
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .unwrap_or("Custom Font")
                    .into()
            });
            let path = new_path("fonts", &extension(&file.name, "ttf"))?;
            assets.write(Path::new(&path), &bytes)?;
            Ok(
                json!({"success":true,"fontName":name,"fontPath":path,"weightRanges":metadata.weight_ranges}),
            )
        }
        "sound_load" => {
            let path = new_path("sounds", &extension(&file.name, "wav"))?;
            assets.write(Path::new(&path), &bytes)?;
            store.sound_library.insert(
                path.clone(),
                SoundLibraryEntry {
                    source: SoundSource::Local,
                    ..Default::default()
                },
            );
            Ok(json!({"success":true,"soundPath":path}))
        }
        _ => Err("unsupported-web-resource-command".into()),
    }
}

#[cfg(test)]
mod tests;
