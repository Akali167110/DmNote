#[cfg(test)]
use crate::models::{GraphPositions, KeyPositions, KnobPositions, SpritePositions, StatPositions};
#[cfg(test)]
use dmnote_editor_engine::preset::{
    export as shared_export, EmbeddedLocalImage, EmbeddedLocalSound, PresetFile,
    PRESET_LOCAL_IMAGE_PREFIX, PRESET_LOCAL_SOUND_PREFIX,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

use tauri::{AppHandle, Manager, WebviewWindow};

use crate::{
    commands::dialog::parented_file_dialog,
    errors::{CmdResult, CommandError},
    state::{atomic_file::atomic_replace, AppState},
};

use super::{local_source_path_from_image_ref, PresetOperationResult};

#[tauri::command]
pub async fn preset_save(
    app: AppHandle,
    window: WebviewWindow,
) -> CmdResult<PresetOperationResult> {
    let preset_file = parented_file_dialog(&window, "DM NOTE Preset", &["json"])
        .set_file_name("preset.json")
        .save_file()
        .await;

    let Some(file) = preset_file else {
        return Ok(PresetOperationResult {
            success: false,
            error: None,
        });
    };
    let path = file.path().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || preset_save_from_path(app, path))
        .await
        .map_err(|error| CommandError::msg(format!("preset export task failed: {error}")))?
}

fn preset_save_from_path(app: AppHandle, path: PathBuf) -> CmdResult<PresetOperationResult> {
    let state = app.state::<AppState>();

    let preset = dmnote_editor_engine::preset::plan::build_full_preset(
        state.store.snapshot(),
        &NativePresetAssets,
    )
    .map_err(CommandError::msg)?;

    let json = serde_json::to_string_pretty(&preset)?;
    write_preset_file(&path, &json)?;

    Ok(PresetOperationResult {
        success: true,
        error: None,
    })
}

#[tauri::command]
pub async fn preset_save_tab(
    app: AppHandle,
    window: WebviewWindow,
) -> CmdResult<PresetOperationResult> {
    let preset_file = parented_file_dialog(&window, "DM NOTE Preset", &["json"])
        .set_file_name("preset-tab.json")
        .save_file()
        .await;

    let Some(file) = preset_file else {
        return Ok(PresetOperationResult {
            success: false,
            error: None,
        });
    };
    let path = file.path().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || preset_save_tab_from_path(app, path))
        .await
        .map_err(|error| CommandError::msg(format!("tab preset export task failed: {error}")))?
}

fn preset_save_tab_from_path(app: AppHandle, path: PathBuf) -> CmdResult<PresetOperationResult> {
    let state = app.state::<AppState>();

    let preset = dmnote_editor_engine::preset::plan::build_tab_preset(
        state.store.snapshot(),
        &NativePresetAssets,
    )
    .map_err(CommandError::msg)?;

    let json = serde_json::to_string_pretty(&preset)?;
    write_preset_file(&path, &json)?;

    Ok(PresetOperationResult {
        success: true,
        error: None,
    })
}

fn write_preset_file(path: &Path, json: &str) -> CmdResult<()> {
    atomic_replace(path, json.as_bytes(), "preset")?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn write_preset_file_for_simulation(path: &Path, preset: &PresetFile) -> CmdResult<()> {
    let json = serde_json::to_string_pretty(preset)?;
    write_preset_file(path, &json)
}

use dmnote_editor_engine::preset::export::PresetAssetReader;
struct NativePresetAssets;
impl PresetAssetReader for NativePresetAssets {
    fn read(&self, path: &Path) -> Result<Vec<u8>, String> {
        fs::read(path).map_err(|error| error.to_string())
    }
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
    fn is_local(&self, path: &Path) -> bool {
        path.is_absolute()
    }
    fn image_source(&self, reference: &str) -> Option<PathBuf> {
        local_source_path_from_image_ref(reference)
    }
}
#[cfg(test)]
pub(super) fn build_preset_image_payload(
    key_positions: &KeyPositions,
    stat_positions: &StatPositions,
    graph_positions: &GraphPositions,
    knob_positions: &KnobPositions,
    sprite_positions: &SpritePositions,
) -> CmdResult<(
    KeyPositions,
    StatPositions,
    GraphPositions,
    KnobPositions,
    SpritePositions,
    Vec<EmbeddedLocalImage>,
)> {
    shared_export::build_preset_image_payload(
        &NativePresetAssets,
        key_positions,
        stat_positions,
        graph_positions,
        knob_positions,
        sprite_positions,
    )
    .map_err(CommandError::msg)
}
#[cfg(test)]
fn build_preset_sound_payload(
    key_positions: &KeyPositions,
    stat_positions: &StatPositions,
    graph_positions: &GraphPositions,
    knob_positions: &KnobPositions,
) -> CmdResult<(
    KeyPositions,
    StatPositions,
    GraphPositions,
    KnobPositions,
    Vec<EmbeddedLocalSound>,
)> {
    shared_export::build_preset_sound_payload(
        &NativePresetAssets,
        key_positions,
        stat_positions,
        graph_positions,
        knob_positions,
    )
    .map_err(CommandError::msg)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{KeyPosition, KnobPosition};
    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};

    #[test]
    fn image_payload_embeds_percent_encoded_file_url() {
        let temp_dir = std::env::temp_dir().join(format!(
            "dmnote-preset-image-url-save-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let image_path = temp_dir.join("image with space.png");
        std::fs::write(&image_path, b"image-bytes").unwrap();
        let image_url = url::Url::from_file_path(&image_path).unwrap().to_string();
        assert!(image_url.contains("%20"));

        let position = KeyPosition {
            active_image: Some(image_url),
            ..KeyPosition::default()
        };
        let key_positions = KeyPositions::from([("4key".to_string(), vec![position])]);

        let (exported, _, _, _, _, embedded) = build_preset_image_payload(
            &key_positions,
            &StatPositions::new(),
            &GraphPositions::new(),
            &KnobPositions::new(),
            &SpritePositions::new(),
        )
        .unwrap();

        assert_eq!(embedded.len(), 1);
        assert_eq!(
            BASE64_STANDARD.decode(&embedded[0].data_base64).unwrap(),
            b"image-bytes"
        );
        assert_eq!(
            exported["4key"][0].active_image.as_deref(),
            Some(format!("{PRESET_LOCAL_IMAGE_PREFIX}{}", embedded[0].image_id).as_str())
        );
        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn sound_payload_embeds_knob_sound() {
        let temp_dir = std::env::temp_dir().join(format!(
            "dmnote-preset-knob-save-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let sound_path = temp_dir.join("knob.wav");
        std::fs::write(&sound_path, b"knob-sound").unwrap();

        let position = KeyPosition {
            sound_path: Some(sound_path.to_string_lossy().to_string()),
            ..KeyPosition::default()
        };
        let mut knob_positions = KnobPositions::new();
        knob_positions.insert(
            "4key".to_string(),
            vec![KnobPosition {
                axis_id: "axis".to_string(),
                sensitivity: 1.0,
                reverse: false,
                position,
            }],
        );

        let (_, _, _, exported_knobs, embedded) = build_preset_sound_payload(
            &KeyPositions::new(),
            &StatPositions::new(),
            &GraphPositions::new(),
            &knob_positions,
        )
        .unwrap();

        assert_eq!(embedded.len(), 1);
        let sound_ref = exported_knobs["4key"][0]
            .position
            .sound_path
            .as_deref()
            .unwrap();
        let sound_id = sound_ref.strip_prefix(PRESET_LOCAL_SOUND_PREFIX).unwrap();
        assert_eq!(sound_id, embedded[0].sound_id);
        assert_eq!(
            BASE64_STANDARD.decode(&embedded[0].data_base64).unwrap(),
            b"knob-sound"
        );
        let _ = std::fs::remove_dir_all(temp_dir);
    }

    // 단독 실행: cargo test --lib commands::preset::save::tests::preset_atomic_write_survives_file_size_limit -- --ignored --exact
    #[cfg(unix)]
    #[test]
    #[ignore = "RLIMIT_FSIZE는 프로세스 전역이므로 단독 실행"]
    fn preset_atomic_write_survives_file_size_limit() {
        use crate::state::atomic_file::test_support::FileSizeLimit;

        let temp_dir = std::env::temp_dir().join(format!(
            "dmnote-preset-rlimit-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let path = temp_dir.join("preset.json");
        let original = vec![b'o'; 512];
        std::fs::write(&path, &original).unwrap();

        {
            let _limit = FileSizeLimit::set(1_024);
            let oversized = "x".repeat(4_096);
            assert!(write_preset_file(&path, &oversized).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), original);
        }

        assert!(!std::fs::read_dir(&temp_dir).unwrap().any(|entry| {
            entry
                .ok()
                .and_then(|entry| entry.file_name().into_string().ok())
                .is_some_and(|name| name.ends_with(".tmp"))
        }));
        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
