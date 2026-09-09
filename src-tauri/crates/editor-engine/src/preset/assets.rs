use super::*;
use crate::models::*;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
use uuid::Uuid;
/// 바이트 보관, 이미지 처리, 로컬 참조 해석은 환경별 호스트의 책임이다.
pub trait PresetAssetWriter {
    type Error: std::fmt::Display;
    fn create_dir_all(&self, path: &Path) -> Result<(), Self::Error>;
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), Self::Error>;
    fn exists(&self, path: &Path) -> bool;
    fn is_local(&self, path: &Path) -> bool;
    fn image_source(&self, reference: &str) -> Option<PathBuf>;
    fn import_image_bytes(
        &self,
        bytes: &[u8],
        directory: &Path,
        extension: &str,
    ) -> Result<PathBuf, Self::Error>;
    fn import_image_file(
        &self,
        source: &Path,
        directory: &Path,
        extension: &str,
    ) -> Result<PathBuf, Self::Error>;
    fn fill_missing_sprite_image_metrics(&self, positions: &mut SpritePositions);
}
pub fn migrate_imported_font_weights(
    key_positions: &mut KeyPositions,
    stat_positions: &mut StatPositions,
    graph_positions: &mut GraphPositions,
    knob_positions: &mut KnobPositions,
) {
    for position in key_positions.values_mut().flatten() {
        position.migrate_legacy_font_weight();
    }
    for position in stat_positions.values_mut().flatten() {
        position.position.migrate_legacy_font_weight();
    }
    for position in graph_positions.values_mut().flatten() {
        position.position.migrate_legacy_font_weight();
    }
    for position in knob_positions.values_mut().flatten() {
        position.position.migrate_legacy_font_weight();
    }
}

#[cfg(any(test, feature = "test-support"))]
pub fn merge_tab_preset_fonts<E>(
    existing_font_settings: &FontSettings,
    imported_font_settings: FontSettings,
    restore_fonts: impl FnOnce(&mut FontSettings) -> Result<(), E>,
) -> Result<Option<FontSettings>, E> {
    let Some(prepared) = prepare_tab_preset_fonts(
        existing_font_settings,
        imported_font_settings,
        restore_fonts,
    )?
    else {
        return Ok(None);
    };
    Ok(merge_prepared_tab_preset_fonts(
        existing_font_settings,
        prepared,
    ))
}

pub fn prepare_tab_preset_fonts<E>(
    existing_font_settings: &FontSettings,
    mut imported_font_settings: FontSettings,
    restore_fonts: impl FnOnce(&mut FontSettings) -> Result<(), E>,
) -> Result<Option<FontSettings>, E> {
    let existing_names: HashSet<String> = existing_font_settings
        .custom_fonts
        .iter()
        .map(|font| font.name.clone())
        .collect();

    // 같은 이름은 기존 정의 유지. 같은 family의 다른 페이스(파일)는 개별 자산이라
    // 이름으로 묶지 않고, 프리셋 내부 중복은 id 기준으로만 방어
    let mut seen_ids: HashSet<String> = HashSet::new();
    imported_font_settings.custom_fonts.retain(|font| {
        !existing_names.contains(&font.name)
            && (font.id.is_empty() || seen_ids.insert(font.id.clone()))
    });
    if imported_font_settings.custom_fonts.is_empty() {
        return Ok(None);
    }

    // 이름 필터 후 파일 복원 — 제외할 로컬 폰트의 고아 파일 생성 방지
    restore_fonts(&mut imported_font_settings)?;

    let mut existing_ids: HashSet<String> = existing_font_settings
        .custom_fonts
        .iter()
        .map(|font| font.id.clone())
        .collect();
    for font in imported_font_settings.custom_fonts.iter_mut() {
        if existing_ids.contains(&font.id) {
            font.id = Uuid::new_v4().to_string();
        }
        existing_ids.insert(font.id.clone());
    }

    Ok(Some(imported_font_settings))
}

pub fn merge_prepared_tab_preset_fonts(
    existing_font_settings: &FontSettings,
    mut prepared: FontSettings,
) -> Option<FontSettings> {
    let existing_names = existing_font_settings
        .custom_fonts
        .iter()
        .map(|font| font.name.clone())
        .collect::<HashSet<_>>();
    let importable_names = prepared
        .custom_fonts
        .iter()
        .filter(|font| !existing_names.contains(&font.name))
        .map(|font| font.name.clone())
        .collect::<HashSet<_>>();
    prepared
        .custom_fonts
        .retain(|font| importable_names.contains(&font.name));
    if prepared.custom_fonts.is_empty() {
        return None;
    }

    let mut existing_ids = existing_font_settings
        .custom_fonts
        .iter()
        .map(|font| font.id.clone())
        .collect::<HashSet<_>>();
    for font in &mut prepared.custom_fonts {
        if existing_ids.contains(&font.id) {
            font.id = Uuid::new_v4().to_string();
        }
        existing_ids.insert(font.id.clone());
    }

    let mut merged = existing_font_settings.clone();
    merged.custom_fonts.extend(prepared.custom_fonts);
    Some(merged)
}

pub fn restore_preset_local_fonts_in_dir<A: PresetAssetWriter>(
    assets: &A,
    fonts_dir: &Path,
    font_settings: &mut FontSettings,
    embedded_local_fonts: Option<&[EmbeddedLocalFont]>,
) -> Result<(), A::Error> {
    let has_local_fonts = font_settings
        .custom_fonts
        .iter()
        .any(|font| font.font_type == FontType::Local);
    if !has_local_fonts {
        return Ok(());
    }

    let embedded_map: HashMap<&str, &EmbeddedLocalFont> = embedded_local_fonts
        .unwrap_or(&[])
        .iter()
        .map(|font| (font.font_id.as_str(), font))
        .collect();

    assets.create_dir_all(fonts_dir)?;

    for font in font_settings.custom_fonts.iter_mut() {
        if font.font_type != FontType::Local {
            continue;
        }

        // 로컬 폰트는 항상 복사된 파일 경로로 제공
        font.css_content = None;

        if let Some(embedded) = embedded_map.get(font.id.as_str()) {
            let bytes = match BASE64_STANDARD.decode(embedded.data_base64.as_bytes()) {
                Ok(bytes) => bytes,
                Err(err) => {
                    log::warn!(
                        "[Preset] Failed to decode embedded local font '{}': {err}",
                        font.display_name
                    );
                    font.local_path = None;
                    font.enabled = false;
                    continue;
                }
            };

            let extension = normalize_font_extension(embedded.extension.as_deref());
            let dest_path = fonts_dir.join(format!("{}.{}", Uuid::new_v4(), extension));
            if let Err(err) = assets.write(&dest_path, &bytes) {
                log::warn!(
                    "[Preset] Failed to restore local font file for '{}': {err}",
                    font.display_name
                );
                font.local_path = None;
                font.enabled = false;
                continue;
            }
            font.local_path = Some(dest_path.to_string_lossy().to_string());
            continue;
        }

        // 하위 호환: 기존 절대 경로가 유효하면 유지
        let has_existing_valid_path = font
            .local_path
            .as_ref()
            .map(|path| !path.trim().is_empty() && assets.exists(Path::new(path)))
            .unwrap_or(false);

        if !has_existing_valid_path {
            log::warn!(
                "[Preset] Disabling font '{}' — no embedded payload and its file is missing on this machine",
                font.name
            );
            font.local_path = None;
            font.enabled = false;
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn restore_preset_local_images_in_dir<A: PresetAssetWriter>(
    assets: &A,
    images_dir: &Path,
    key_positions: &mut KeyPositions,
    stat_positions: &mut StatPositions,
    graph_positions: &mut GraphPositions,
    knob_positions: &mut KnobPositions,
    sprite_positions: &mut SpritePositions,
    embedded_local_images: Option<&[EmbeddedLocalImage]>,
) -> Result<(), A::Error> {
    let has_any_images = key_positions.values().any(|positions| {
        positions.iter().any(|position| {
            option_has_non_empty_text(&position.active_image)
                || option_has_non_empty_text(&position.inactive_image)
        })
    }) || stat_positions.values().any(|positions| {
        positions.iter().any(|stat_position| {
            option_has_non_empty_text(&stat_position.position.active_image)
                || option_has_non_empty_text(&stat_position.position.inactive_image)
        })
    }) || graph_positions.values().any(|positions| {
        positions.iter().any(|graph_position| {
            option_has_non_empty_text(&graph_position.position.active_image)
                || option_has_non_empty_text(&graph_position.position.inactive_image)
        })
    }) || knob_positions.values().any(|positions| {
        positions.iter().any(|knob_position| {
            option_has_non_empty_text(&knob_position.position.active_image)
                || option_has_non_empty_text(&knob_position.position.inactive_image)
        })
    }) || sprite_positions.values().any(|sprites| {
        sprites.iter().any(|sprite| {
            option_has_non_empty_text(&sprite.base_image)
                || sprite
                    .poses
                    .iter()
                    .any(|pose| option_has_non_empty_text(&pose.image_override))
        })
    });

    if !has_any_images {
        return Ok(());
    }

    let embedded_map: HashMap<&str, &EmbeddedLocalImage> = embedded_local_images
        .unwrap_or(&[])
        .iter()
        .map(|image| (image.image_id.as_str(), image))
        .collect();
    let mut restored_path_cache: HashMap<String, String> = HashMap::new();

    assets.create_dir_all(images_dir)?;

    for positions in key_positions.values_mut() {
        for position in positions.iter_mut() {
            restore_position_image_reference(
                assets,
                images_dir,
                &embedded_map,
                &mut restored_path_cache,
                &mut position.active_image,
            )?;
            restore_position_image_reference(
                assets,
                images_dir,
                &embedded_map,
                &mut restored_path_cache,
                &mut position.inactive_image,
            )?;
        }
    }

    for positions in stat_positions.values_mut() {
        for stat_position in positions.iter_mut() {
            restore_position_image_reference(
                assets,
                images_dir,
                &embedded_map,
                &mut restored_path_cache,
                &mut stat_position.position.active_image,
            )?;
            restore_position_image_reference(
                assets,
                images_dir,
                &embedded_map,
                &mut restored_path_cache,
                &mut stat_position.position.inactive_image,
            )?;
        }
    }

    for positions in graph_positions.values_mut() {
        for graph_position in positions.iter_mut() {
            restore_position_image_reference(
                assets,
                images_dir,
                &embedded_map,
                &mut restored_path_cache,
                &mut graph_position.position.active_image,
            )?;
            restore_position_image_reference(
                assets,
                images_dir,
                &embedded_map,
                &mut restored_path_cache,
                &mut graph_position.position.inactive_image,
            )?;
        }
    }

    for positions in knob_positions.values_mut() {
        for knob_position in positions.iter_mut() {
            restore_position_image_reference(
                assets,
                images_dir,
                &embedded_map,
                &mut restored_path_cache,
                &mut knob_position.position.active_image,
            )?;
            restore_position_image_reference(
                assets,
                images_dir,
                &embedded_map,
                &mut restored_path_cache,
                &mut knob_position.position.inactive_image,
            )?;
        }
    }

    for sprites in sprite_positions.values_mut() {
        for sprite in sprites {
            rewrite_coupled_sprite_image_reference(sprite, |image_ref| {
                restore_position_image_reference(
                    assets,
                    images_dir,
                    &embedded_map,
                    &mut restored_path_cache,
                    image_ref,
                )
            })?;
            for pose in &mut sprite.poses {
                rewrite_coupled_sprite_image_reference(pose, |image_ref| {
                    restore_position_image_reference(
                        assets,
                        images_dir,
                        &embedded_map,
                        &mut restored_path_cache,
                        image_ref,
                    )
                })?;
            }
        }
    }

    assets.fill_missing_sprite_image_metrics(sprite_positions);

    Ok(())
}

pub fn restore_position_image_reference<A: PresetAssetWriter>(
    assets: &A,
    images_dir: &Path,
    embedded_map: &HashMap<&str, &EmbeddedLocalImage>,
    restored_path_cache: &mut HashMap<String, String>,
    image_ref: &mut Option<String>,
) -> Result<(), A::Error> {
    let Some(current_value) = image_ref.clone() else {
        return Ok(());
    };
    let trimmed = current_value.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    if let Some(image_id) = trimmed.strip_prefix(PRESET_LOCAL_IMAGE_PREFIX) {
        if let Some(restored_path) = restored_path_cache.get(image_id) {
            *image_ref = Some(restored_path.clone());
            return Ok(());
        }
        let Some(embedded) = embedded_map.get(image_id) else {
            log::warn!(
                "[Preset] Missing embedded image payload for id '{}'; clearing image reference",
                image_id
            );
            *image_ref = None;
            return Ok(());
        };

        let bytes = match BASE64_STANDARD.decode(embedded.data_base64.as_bytes()) {
            Ok(bytes) => bytes,
            Err(err) => {
                log::warn!(
                    "[Preset] Failed to decode embedded image '{}': {err}",
                    image_id
                );
                *image_ref = None;
                return Ok(());
            }
        };
        let extension = normalize_image_extension(embedded.extension.as_deref());
        let imported = match assets.import_image_bytes(&bytes, images_dir, &extension) {
            Ok(imported) => imported,
            Err(err) => {
                log::warn!(
                    "[Preset] Failed to restore embedded image '{}': {err}",
                    image_id
                );
                *image_ref = None;
                return Ok(());
            }
        };
        let restored = imported.to_string_lossy().to_string();
        restored_path_cache.insert(image_id.to_string(), restored.clone());
        *image_ref = Some(restored);
        return Ok(());
    }

    // 레거시 Preset 호환: data URL 이미지를 appdata 파일 경로로 변환
    if let Some((bytes, extension)) = decode_image_data_url(trimmed) {
        let imported = assets.import_image_bytes(&bytes, images_dir, &extension)?;
        *image_ref = Some(imported.to_string_lossy().to_string());
        return Ok(());
    }

    // 레거시 호환: 로컬 절대 경로를 appdata/images로 복사
    if let Some(source_path) = assets.image_source(trimmed) {
        if assets.exists(&source_path) {
            if source_path.starts_with(images_dir) {
                *image_ref = Some(source_path.to_string_lossy().to_string());
                return Ok(());
            }
            let extension =
                normalize_image_extension(source_path.extension().and_then(|ext| ext.to_str()));
            let imported = match assets.import_image_file(&source_path, images_dir, &extension) {
                Ok(imported) => imported,
                Err(err) => {
                    log::warn!(
                        "[Preset] Failed to copy local image from '{}': {err}",
                        source_path.display()
                    );
                    *image_ref = None;
                    return Ok(());
                }
            };
            *image_ref = Some(imported.to_string_lossy().to_string());
            return Ok(());
        }

        // 다른 기기에서 import된 Preset: 해석 불가한 절대 경로는 정상 fallback 처리
        log::warn!(
            "[Preset] Clearing image reference to a file missing on this machine: {trimmed}"
        );
        *image_ref = None;
        return Ok(());
    }

    Ok(())
}

pub fn restore_preset_local_sounds_in_dir<A: PresetAssetWriter>(
    assets: &A,
    sounds_dir: &Path,
    key_positions: &mut KeyPositions,
    stat_positions: &mut StatPositions,
    graph_positions: &mut GraphPositions,
    knob_positions: &mut KnobPositions,
    embedded_local_sounds: Option<&[EmbeddedLocalSound]>,
) -> Result<(), A::Error> {
    assets.create_dir_all(sounds_dir)?;

    let embedded_map: HashMap<&str, &EmbeddedLocalSound> = embedded_local_sounds
        .unwrap_or(&[])
        .iter()
        .map(|sound| (sound.sound_id.as_str(), sound))
        .collect();

    let mut restored_path_cache: HashMap<String, String> = HashMap::new();

    for positions in key_positions.values_mut() {
        for position in positions.iter_mut() {
            restore_position_sound_reference(
                assets,
                sounds_dir,
                &embedded_map,
                &mut restored_path_cache,
                &mut position.sound_path,
            )?;
        }
    }

    for positions in stat_positions.values_mut() {
        for stat_position in positions.iter_mut() {
            restore_position_sound_reference(
                assets,
                sounds_dir,
                &embedded_map,
                &mut restored_path_cache,
                &mut stat_position.position.sound_path,
            )?;
        }
    }

    for positions in graph_positions.values_mut() {
        for graph_position in positions.iter_mut() {
            restore_position_sound_reference(
                assets,
                sounds_dir,
                &embedded_map,
                &mut restored_path_cache,
                &mut graph_position.position.sound_path,
            )?;
        }
    }

    for positions in knob_positions.values_mut() {
        for knob_position in positions.iter_mut() {
            restore_position_sound_reference(
                assets,
                sounds_dir,
                &embedded_map,
                &mut restored_path_cache,
                &mut knob_position.position.sound_path,
            )?;
        }
    }

    Ok(())
}

pub fn restore_position_sound_reference<A: PresetAssetWriter>(
    assets: &A,
    sounds_dir: &Path,
    embedded_map: &HashMap<&str, &EmbeddedLocalSound>,
    restored_path_cache: &mut HashMap<String, String>,
    sound_ref: &mut Option<String>,
) -> Result<(), A::Error> {
    let Some(current_value) = sound_ref.clone() else {
        return Ok(());
    };
    let trimmed = current_value.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    if let Some(sound_id) = trimmed.strip_prefix(PRESET_LOCAL_SOUND_PREFIX) {
        if let Some(restored_path) = restored_path_cache.get(sound_id) {
            *sound_ref = Some(restored_path.clone());
            return Ok(());
        }
        let Some(embedded) = embedded_map.get(sound_id) else {
            log::warn!(
                "[Preset] Missing embedded sound payload for id '{}'; clearing sound reference",
                sound_id
            );
            *sound_ref = None;
            return Ok(());
        };

        let bytes = match BASE64_STANDARD.decode(embedded.data_base64.as_bytes()) {
            Ok(bytes) => bytes,
            Err(err) => {
                log::warn!(
                    "[Preset] Failed to decode embedded sound '{}': {err}",
                    sound_id
                );
                *sound_ref = None;
                return Ok(());
            }
        };

        let extension = normalize_sound_extension(embedded.extension.as_deref());
        let dest_path = sounds_dir.join(format!("{}.{}", Uuid::new_v4(), extension));
        if let Err(err) = assets.write(&dest_path, &bytes) {
            log::warn!(
                "[Preset] Failed to restore embedded sound '{}': {err}",
                sound_id
            );
            *sound_ref = None;
            return Ok(());
        }
        let restored = dest_path.to_string_lossy().to_string();
        restored_path_cache.insert(sound_id.to_string(), restored.clone());
        *sound_ref = Some(restored);
        return Ok(());
    }

    // 레거시 호환: 절대 경로가 현재 기기에서 유효하면 그대로 유지.
    let path = std::path::PathBuf::from(trimmed);
    if assets.is_local(&path) && assets.exists(&path) {
        return Ok(());
    }

    // 다른 기기에서 임포트된 프리셋: 경로를 해석할 수 없으면 초기화.
    log::warn!("[Preset] Clearing sound reference to a file missing on this machine: {trimmed}");
    *sound_ref = None;
    Ok(())
}
