use super::*;
use dmnote_editor_engine::preset::assets::{self as shared_assets, PresetAssetWriter};
#[cfg(test)]
pub(super) use shared_assets::prepare_tab_preset_fonts;
#[cfg(test)]
pub(super) fn merge_tab_preset_fonts(
    existing: &FontSettings,
    imported: FontSettings,
    restore: impl FnOnce(&mut FontSettings) -> CmdResult<()>,
) -> CmdResult<Option<FontSettings>> {
    shared_assets::merge_tab_preset_fonts(existing, imported, restore)
}
struct NativePresetAssetWriter;
impl PresetAssetWriter for NativePresetAssetWriter {
    type Error = CommandError;
    fn create_dir_all(&self, path: &Path) -> CmdResult<()> {
        fs::create_dir_all(path)?;
        Ok(())
    }
    fn write(&self, path: &Path, bytes: &[u8]) -> CmdResult<()> {
        fs::write(path, bytes)?;
        Ok(())
    }
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
    fn is_local(&self, path: &Path) -> bool {
        path.is_absolute()
    }
    fn image_source(&self, reference: &str) -> Option<PathBuf> {
        super::super::local_source_path_from_image_ref(reference)
    }
    fn import_image_bytes(
        &self,
        bytes: &[u8],
        directory: &Path,
        extension: &str,
    ) -> CmdResult<PathBuf> {
        Ok(import_image_bytes(bytes, directory, extension)?.path)
    }
    fn import_image_file(
        &self,
        source: &Path,
        directory: &Path,
        extension: &str,
    ) -> CmdResult<PathBuf> {
        Ok(import_image_file(source, directory, extension)?.path)
    }
    fn fill_missing_sprite_image_metrics(&self, positions: &mut SpritePositions) {
        fill_missing_sprite_image_metrics(positions);
    }
}
pub(super) fn restore_preset_local_fonts(
    app: &AppHandle,
    font_settings: &mut FontSettings,
    embedded_local_fonts: Option<&[EmbeddedLocalFont]>,
) -> CmdResult<()> {
    let has_local_fonts = font_settings
        .custom_fonts
        .iter()
        .any(|font| font.font_type == FontType::Local);
    if !has_local_fonts {
        return Ok(());
    }

    let app_data_dir = app.path().app_data_dir()?;
    let fonts_dir = app_data_dir.join("fonts");

    restore_preset_local_fonts_in_dir(&fonts_dir, font_settings, embedded_local_fonts)
}

pub(super) fn restore_preset_local_images(
    app: &AppHandle,
    key_positions: &mut KeyPositions,
    stat_positions: &mut StatPositions,
    graph_positions: &mut GraphPositions,
    knob_positions: &mut KnobPositions,
    sprite_positions: &mut SpritePositions,
    embedded_local_images: Option<&[EmbeddedLocalImage]>,
) -> CmdResult<()> {
    let app_data_dir = app.path().app_data_dir()?;
    restore_preset_local_images_in_dir(
        &app_data_dir.join("images"),
        key_positions,
        stat_positions,
        graph_positions,
        knob_positions,
        sprite_positions,
        embedded_local_images,
    )
}

pub(super) fn restore_preset_local_sounds(
    app: &AppHandle,
    key_positions: &mut KeyPositions,
    stat_positions: &mut StatPositions,
    graph_positions: &mut GraphPositions,
    knob_positions: &mut KnobPositions,
    embedded_local_sounds: Option<&[EmbeddedLocalSound]>,
) -> CmdResult<()> {
    let has_any_sounds = key_positions.values().any(|positions| {
        positions
            .iter()
            .any(|position| option_has_non_empty_text(&position.sound_path))
    }) || stat_positions.values().any(|positions| {
        positions
            .iter()
            .any(|stat_position| option_has_non_empty_text(&stat_position.position.sound_path))
    }) || graph_positions.values().any(|positions| {
        positions
            .iter()
            .any(|graph_position| option_has_non_empty_text(&graph_position.position.sound_path))
    }) || knob_positions.values().any(|positions| {
        positions
            .iter()
            .any(|knob_position| option_has_non_empty_text(&knob_position.position.sound_path))
    });

    if !has_any_sounds {
        return Ok(());
    }

    let app_data_dir = app.path().app_data_dir()?;
    let sounds_dir = app_data_dir.join("sounds");

    restore_preset_local_sounds_in_dir(
        &sounds_dir,
        key_positions,
        stat_positions,
        graph_positions,
        knob_positions,
        embedded_local_sounds,
    )
}

pub(super) fn restore_preset_local_fonts_in_dir(
    fonts_dir: &Path,
    font_settings: &mut FontSettings,
    embedded_local_fonts: Option<&[EmbeddedLocalFont]>,
) -> CmdResult<()> {
    shared_assets::restore_preset_local_fonts_in_dir(
        &NativePresetAssetWriter,
        fonts_dir,
        font_settings,
        embedded_local_fonts,
    )
}
pub(super) fn restore_preset_local_images_in_dir(
    images_dir: &Path,
    key_positions: &mut KeyPositions,
    stat_positions: &mut StatPositions,
    graph_positions: &mut GraphPositions,
    knob_positions: &mut KnobPositions,
    sprite_positions: &mut SpritePositions,
    embedded_local_images: Option<&[EmbeddedLocalImage]>,
) -> CmdResult<()> {
    shared_assets::restore_preset_local_images_in_dir(
        &NativePresetAssetWriter,
        images_dir,
        key_positions,
        stat_positions,
        graph_positions,
        knob_positions,
        sprite_positions,
        embedded_local_images,
    )
}
#[cfg(test)]
pub(super) fn restore_position_image_reference(
    images_dir: &Path,
    embedded_map: &HashMap<&str, &EmbeddedLocalImage>,
    restored_path_cache: &mut HashMap<String, String>,
    image_ref: &mut Option<String>,
) -> CmdResult<()> {
    shared_assets::restore_position_image_reference(
        &NativePresetAssetWriter,
        images_dir,
        embedded_map,
        restored_path_cache,
        image_ref,
    )
}
pub(super) fn restore_preset_local_sounds_in_dir(
    sounds_dir: &Path,
    key_positions: &mut KeyPositions,
    stat_positions: &mut StatPositions,
    graph_positions: &mut GraphPositions,
    knob_positions: &mut KnobPositions,
    embedded_local_sounds: Option<&[EmbeddedLocalSound]>,
) -> CmdResult<()> {
    shared_assets::restore_preset_local_sounds_in_dir(
        &NativePresetAssetWriter,
        sounds_dir,
        key_positions,
        stat_positions,
        graph_positions,
        knob_positions,
        embedded_local_sounds,
    )
}
