use super::*;
use crate::models::*;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
type ExportResult<T> = Result<T, String>;
/// 자산 표현과 바이트 입출력은 호스트가 제공한다.
pub trait PresetAssetReader {
    fn read(&self, path: &Path) -> Result<Vec<u8>, String>;
    fn exists(&self, path: &Path) -> bool;
    fn is_local(&self, path: &Path) -> bool;
    fn image_source(&self, reference: &str) -> Option<PathBuf>;
}
pub fn collect_used_font_families(
    key_positions: &KeyPositions,
    stat_positions: &StatPositions,
    graph_positions: &GraphPositions,
    knob_positions: &KnobPositions,
) -> HashSet<String> {
    let mut used = HashSet::new();

    for positions in key_positions.values() {
        for position in positions {
            maybe_insert_font_family(position.font_family.as_ref(), &mut used);
            maybe_insert_font_family(position.counter.font_family.as_ref(), &mut used);
        }
    }

    for positions in stat_positions.values() {
        for stat_position in positions {
            maybe_insert_font_family(stat_position.position.font_family.as_ref(), &mut used);
            maybe_insert_font_family(
                stat_position.position.counter.font_family.as_ref(),
                &mut used,
            );
        }
    }

    for positions in graph_positions.values() {
        for graph_position in positions {
            maybe_insert_font_family(graph_position.position.font_family.as_ref(), &mut used);
            maybe_insert_font_family(
                graph_position.position.counter.font_family.as_ref(),
                &mut used,
            );
        }
    }

    for positions in knob_positions.values() {
        for knob_position in positions {
            maybe_insert_font_family(knob_position.position.font_family.as_ref(), &mut used);
            maybe_insert_font_family(
                knob_position.position.counter.font_family.as_ref(),
                &mut used,
            );
        }
    }

    used
}

fn maybe_insert_font_family(value: Option<&String>, target: &mut HashSet<String>) {
    if let Some(font_family) = value {
        let trimmed = font_family.trim();
        if !trimmed.is_empty() {
            target.insert(trimmed.to_string());
        }
    }
}

pub fn build_preset_font_payload(
    assets: &impl PresetAssetReader,
    font_settings: &FontSettings,
    used_font_families: &HashSet<String>,
) -> ExportResult<(FontSettings, Vec<EmbeddedLocalFont>)> {
    let mut exported_fonts = Vec::new();
    let mut embedded_local_fonts = Vec::new();

    for font in font_settings.custom_fonts.iter() {
        if !used_font_families.contains(&font.name) {
            continue;
        }

        let mut next_font = font.clone();

        if next_font.font_type == FontType::Local {
            let local_path = match next_font.local_path.clone() {
                Some(path) if !path.trim().is_empty() => path,
                _ => {
                    log::warn!(
                        "[Preset] Local font '{}' has no path; exporting as disabled fallback",
                        next_font.display_name
                    );
                    next_font.local_path = None;
                    next_font.css_content = None;
                    next_font.enabled = false;
                    exported_fonts.push(next_font);
                    continue;
                }
            };

            let source_path = PathBuf::from(local_path);
            let bytes = match assets.read(&source_path) {
                Ok(bytes) => bytes,
                Err(err) => {
                    log::warn!(
                        "[Preset] Failed to read local font '{}' from '{}': {err}. Exporting as disabled fallback",
                        next_font.display_name,
                        source_path.display()
                    );
                    next_font.local_path = None;
                    next_font.css_content = None;
                    next_font.enabled = false;
                    exported_fonts.push(next_font);
                    continue;
                }
            };

            let extension =
                normalize_font_extension(source_path.extension().and_then(|ext| ext.to_str()));
            embedded_local_fonts.push(EmbeddedLocalFont {
                font_id: next_font.id.clone(),
                extension: Some(extension),
                data_base64: BASE64_STANDARD.encode(bytes),
            });

            // Preset 이식성: import 시 경로 재구성
            next_font.local_path = None;
            next_font.css_content = None;
        }

        exported_fonts.push(next_font);
    }

    Ok((
        FontSettings {
            custom_fonts: exported_fonts,
        },
        embedded_local_fonts,
    ))
}

pub fn build_preset_image_payload(
    assets: &impl PresetAssetReader,
    key_positions: &KeyPositions,
    stat_positions: &StatPositions,
    graph_positions: &GraphPositions,
    knob_positions: &KnobPositions,
    sprite_positions: &SpritePositions,
) -> ExportResult<(
    KeyPositions,
    StatPositions,
    GraphPositions,
    KnobPositions,
    SpritePositions,
    Vec<EmbeddedLocalImage>,
)> {
    let mut exported_key_positions = key_positions.clone();
    let mut exported_stat_positions = stat_positions.clone();
    let mut exported_graph_positions = graph_positions.clone();
    let mut exported_knob_positions = knob_positions.clone();
    let mut exported_sprite_positions = sprite_positions.clone();
    let mut embedded_local_images = Vec::new();
    let mut path_to_image_id: HashMap<String, String> = HashMap::new();

    for positions in exported_key_positions.values_mut() {
        for position in positions.iter_mut() {
            rewrite_position_image_reference(
                assets,
                &mut position.active_image,
                &mut embedded_local_images,
                &mut path_to_image_id,
            )?;
            rewrite_position_image_reference(
                assets,
                &mut position.inactive_image,
                &mut embedded_local_images,
                &mut path_to_image_id,
            )?;
        }
    }

    for positions in exported_stat_positions.values_mut() {
        for stat_position in positions.iter_mut() {
            rewrite_position_image_reference(
                assets,
                &mut stat_position.position.active_image,
                &mut embedded_local_images,
                &mut path_to_image_id,
            )?;
            rewrite_position_image_reference(
                assets,
                &mut stat_position.position.inactive_image,
                &mut embedded_local_images,
                &mut path_to_image_id,
            )?;
        }
    }

    for positions in exported_graph_positions.values_mut() {
        for graph_position in positions.iter_mut() {
            rewrite_position_image_reference(
                assets,
                &mut graph_position.position.active_image,
                &mut embedded_local_images,
                &mut path_to_image_id,
            )?;
            rewrite_position_image_reference(
                assets,
                &mut graph_position.position.inactive_image,
                &mut embedded_local_images,
                &mut path_to_image_id,
            )?;
        }
    }

    for positions in exported_knob_positions.values_mut() {
        for knob_position in positions.iter_mut() {
            rewrite_position_image_reference(
                assets,
                &mut knob_position.position.active_image,
                &mut embedded_local_images,
                &mut path_to_image_id,
            )?;
            rewrite_position_image_reference(
                assets,
                &mut knob_position.position.inactive_image,
                &mut embedded_local_images,
                &mut path_to_image_id,
            )?;
        }
    }

    for sprites in exported_sprite_positions.values_mut() {
        for sprite in sprites {
            rewrite_coupled_sprite_image_reference(sprite, |image_ref| {
                rewrite_position_image_reference(
                    assets,
                    image_ref,
                    &mut embedded_local_images,
                    &mut path_to_image_id,
                )
            })?;
            for pose in &mut sprite.poses {
                rewrite_coupled_sprite_image_reference(pose, |image_ref| {
                    rewrite_position_image_reference(
                        assets,
                        image_ref,
                        &mut embedded_local_images,
                        &mut path_to_image_id,
                    )
                })?;
            }
        }
    }

    Ok((
        exported_key_positions,
        exported_stat_positions,
        exported_graph_positions,
        exported_knob_positions,
        exported_sprite_positions,
        embedded_local_images,
    ))
}

fn rewrite_position_image_reference(
    assets: &impl PresetAssetReader,
    image_ref: &mut Option<String>,
    embedded_local_images: &mut Vec<EmbeddedLocalImage>,
    path_to_image_id: &mut HashMap<String, String>,
) -> ExportResult<()> {
    let Some(current_value) = image_ref.clone() else {
        return Ok(());
    };
    let trimmed = current_value.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    // 외부 URL은 그대로 유지
    if is_remote_or_virtual_image_ref(trimmed) {
        return Ok(());
    }

    if let Some((bytes, extension)) = decode_image_data_url(trimmed) {
        let image_id = uuid::Uuid::new_v4().to_string();
        embedded_local_images.push(EmbeddedLocalImage {
            image_id: image_id.clone(),
            extension: Some(extension),
            data_base64: BASE64_STANDARD.encode(bytes),
        });
        *image_ref = Some(format!("{PRESET_LOCAL_IMAGE_PREFIX}{image_id}"));
        return Ok(());
    }

    let Some(source_path) = assets.image_source(trimmed) else {
        return Ok(());
    };
    if !assets.exists(&source_path) {
        // 실물 없는 참조는 임베드 불가 — 진단용 흔적만 남김
        log::warn!(
            "[Preset] Skipping image embed for a file missing on this machine: {}",
            source_path.display()
        );
        return Ok(());
    }

    let source_key = source_path.to_string_lossy().to_string();
    if let Some(existing_id) = path_to_image_id.get(&source_key) {
        *image_ref = Some(format!("{PRESET_LOCAL_IMAGE_PREFIX}{existing_id}"));
        return Ok(());
    }

    let bytes = match assets.read(&source_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            log::warn!(
                "[Preset] Failed to read local image from '{}': {err}",
                source_path.display()
            );
            return Ok(());
        }
    };

    let extension = normalize_image_extension(source_path.extension().and_then(|ext| ext.to_str()));
    let image_id = uuid::Uuid::new_v4().to_string();
    embedded_local_images.push(EmbeddedLocalImage {
        image_id: image_id.clone(),
        extension: Some(extension),
        data_base64: BASE64_STANDARD.encode(bytes),
    });
    path_to_image_id.insert(source_key, image_id.clone());
    *image_ref = Some(format!("{PRESET_LOCAL_IMAGE_PREFIX}{image_id}"));
    Ok(())
}

pub fn build_preset_sound_payload(
    assets: &impl PresetAssetReader,
    key_positions: &KeyPositions,
    stat_positions: &StatPositions,
    graph_positions: &GraphPositions,
    knob_positions: &KnobPositions,
) -> ExportResult<(
    KeyPositions,
    StatPositions,
    GraphPositions,
    KnobPositions,
    Vec<EmbeddedLocalSound>,
)> {
    let mut exported_key_positions = key_positions.clone();
    let mut exported_stat_positions = stat_positions.clone();
    let mut exported_graph_positions = graph_positions.clone();
    let mut exported_knob_positions = knob_positions.clone();
    let mut embedded_local_sounds = Vec::new();
    let mut path_to_sound_id: HashMap<String, String> = HashMap::new();

    for positions in exported_key_positions.values_mut() {
        for position in positions.iter_mut() {
            rewrite_position_sound_reference(
                assets,
                &mut position.sound_path,
                &mut embedded_local_sounds,
                &mut path_to_sound_id,
            )?;
        }
    }

    for positions in exported_stat_positions.values_mut() {
        for stat_position in positions.iter_mut() {
            rewrite_position_sound_reference(
                assets,
                &mut stat_position.position.sound_path,
                &mut embedded_local_sounds,
                &mut path_to_sound_id,
            )?;
        }
    }

    for positions in exported_graph_positions.values_mut() {
        for graph_position in positions.iter_mut() {
            rewrite_position_sound_reference(
                assets,
                &mut graph_position.position.sound_path,
                &mut embedded_local_sounds,
                &mut path_to_sound_id,
            )?;
        }
    }

    for positions in exported_knob_positions.values_mut() {
        for knob_position in positions.iter_mut() {
            rewrite_position_sound_reference(
                assets,
                &mut knob_position.position.sound_path,
                &mut embedded_local_sounds,
                &mut path_to_sound_id,
            )?;
        }
    }

    Ok((
        exported_key_positions,
        exported_stat_positions,
        exported_graph_positions,
        exported_knob_positions,
        embedded_local_sounds,
    ))
}

fn rewrite_position_sound_reference(
    assets: &impl PresetAssetReader,
    sound_ref: &mut Option<String>,
    embedded_local_sounds: &mut Vec<EmbeddedLocalSound>,
    path_to_sound_id: &mut HashMap<String, String>,
) -> ExportResult<()> {
    let Some(current_value) = sound_ref.clone() else {
        return Ok(());
    };
    let trimmed = current_value.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    let source_path = PathBuf::from(trimmed);
    if !assets.is_local(&source_path) || !assets.exists(&source_path) {
        // 실물 없는 참조는 임베드 불가 — 진단용 흔적만 남김
        log::warn!("[Preset] Skipping sound embed for a missing or non-absolute path: {trimmed}");
        return Ok(());
    }

    let source_key = source_path.to_string_lossy().to_string();
    if let Some(existing_id) = path_to_sound_id.get(&source_key) {
        *sound_ref = Some(format!("{PRESET_LOCAL_SOUND_PREFIX}{existing_id}"));
        return Ok(());
    }

    let bytes = match assets.read(&source_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            log::warn!(
                "[Preset] Failed to read local sound from '{}': {err}",
                source_path.display()
            );
            return Ok(());
        }
    };

    let extension = normalize_sound_extension(source_path.extension().and_then(|ext| ext.to_str()));
    let sound_id = uuid::Uuid::new_v4().to_string();
    embedded_local_sounds.push(EmbeddedLocalSound {
        sound_id: sound_id.clone(),
        extension: Some(extension),
        data_base64: BASE64_STANDARD.encode(bytes),
    });
    path_to_sound_id.insert(source_key, sound_id.clone());
    *sound_ref = Some(format!("{PRESET_LOCAL_SOUND_PREFIX}{sound_id}"));
    Ok(())
}
