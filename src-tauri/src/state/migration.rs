use crate::custom_css::canonicalize_legacy_css_path;
use anyhow::{Context, Result};
use dirs_next::config_dir;
#[cfg(test)]
use dmnote_editor_engine::{
    portable_assets::{parse_portable_asset_reference, AssetCategory},
    state::migration::test_support::*,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
mod assets;
#[cfg(test)]
use assets::{
    decode_font_data_url, decode_image_data_url, normalize_font_extension,
    normalize_image_extension,
};
pub(crate) use assets::{
    fill_missing_sprite_image_metrics, is_foreign_portable_asset_reference,
    migrate_key_images_to_app_data, migrate_local_fonts_to_app_data,
    rehome_foreign_asset_references,
};
pub use dmnote_editor_engine::state::migration::*;
pub(crate) fn load_store_from_path(path: &Path) -> Result<LoadedStore> {
    let content = fs::read(path)
        .with_context(|| format!("failed to read store file at {}", path.display()))?;
    Ok(load_store_bytes(
        &content,
        &path.display().to_string(),
        current_unix_millis(),
        canonicalize_legacy_css_path,
    ))
}
fn current_unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
/// 레거시 store 파일 경로 탐색
pub(crate) fn find_legacy_store_file() -> Option<PathBuf> {
    // 고정된 레거시 경로: %APPDATA%/dm-note/config.json
    let base = config_dir()?;
    let candidate = base.join("dm-note").join("config.json");
    if candidate.exists() {
        Some(candidate)
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
