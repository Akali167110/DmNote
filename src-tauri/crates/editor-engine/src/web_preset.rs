//! 웹 프리셋은 메모리에서 준비하고 저장소와 자산의 확정은 호스트가 함께 수행한다.
use crate::{
    local_asset_path::{file_url_to_path, FileUrlPath},
    models::*,
    preset::{
        assets::{self, PresetAssetWriter},
        export::PresetAssetReader,
        plan::{self, PresetImportHost},
        EmbeddedLocalFont, EmbeddedLocalImage, EmbeddedLocalSound, PresetFile,
    },
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// 논리 자산 경로와 base64 바이트. 브라우저 Blob URL은 영속 문서에 저장하지 않는다.
pub type WebAssetMap = BTreeMap<String, String>;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebPresetImport {
    pub store: AppStoreData,
    pub asset_writes: WebAssetMap,
    pub settings_diff: Option<SettingsDiff>,
    pub publication: Option<plan::FullPresetPublication>,
}

pub fn is_web_asset_key(key: &str) -> bool {
    let Some(rest) = key.strip_prefix("/assets/") else {
        return false;
    };
    let Some((category, mut file)) = rest.split_once('/') else {
        return false;
    };
    if category == "sounds" {
        file = file.strip_prefix("originals/").unwrap_or(file);
    }
    matches!(category, "images" | "fonts" | "sounds" | "css" | "scripts")
        && !file.is_empty()
        && file != "."
        && file != ".."
        && file
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// 기존 자산은 불변. 새 바이트는 별도 쓰기 집합에만 추가한다.
pub struct WebPresetAssets {
    existing: BTreeMap<String, Vec<u8>>,
    writes: RefCell<BTreeMap<String, Vec<u8>>>,
}
impl WebPresetAssets {
    pub fn new(assets: &WebAssetMap) -> Result<Self, String> {
        let mut existing = BTreeMap::new();
        for (key, data) in assets {
            if !is_web_asset_key(key) {
                return Err(format!("invalid-web-asset-key: {key}"));
            }
            let bytes = BASE64_STANDARD
                .decode(data)
                .map_err(|_| format!("invalid-web-asset-data: {key}"))?;
            existing.insert(key.clone(), bytes);
        }
        Ok(Self {
            existing,
            writes: RefCell::new(BTreeMap::new()),
        })
    }
    pub fn into_writes(self) -> WebAssetMap {
        self.writes
            .into_inner()
            .into_iter()
            .map(|(key, bytes)| (key, BASE64_STANDARD.encode(bytes)))
            .collect()
    }
    fn bytes(&self, path: &Path) -> Result<Vec<u8>, String> {
        let key = path.to_string_lossy();
        self.writes
            .borrow()
            .get(key.as_ref())
            .or_else(|| self.existing.get(key.as_ref()))
            .cloned()
            .ok_or_else(|| format!("missing-web-asset: {key}"))
    }
    fn has(&self, path: &Path) -> bool {
        let key = path.to_string_lossy();
        self.writes.borrow().contains_key(key.as_ref()) || self.existing.contains_key(key.as_ref())
    }
    fn valid_css_path(&self, path: &str) -> bool {
        self.bytes(Path::new(path))
            .is_ok_and(|bytes| crate::web_resources::css::validate_css_bytes(path, &bytes).is_ok())
    }
    fn raster_size(&self, reference: &str) -> Option<(u32, u32)> {
        let path = image_source(reference)?;
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
        {
            return None;
        }
        let size = imagesize::blob_size(&self.bytes(&path).ok()?).ok()?;
        let width = u32::try_from(size.width).ok()?;
        let height = u32::try_from(size.height).ok()?;
        ((SPRITE_IMAGE_DIMENSION_MIN..=SPRITE_IMAGE_DIMENSION_MAX).contains(&width)
            && (SPRITE_IMAGE_DIMENSION_MIN..=SPRITE_IMAGE_DIMENSION_MAX).contains(&height))
        .then_some((width, height))
    }
}
fn image_source(reference: &str) -> Option<PathBuf> {
    let reference = reference.trim();
    match file_url_to_path(reference) {
        FileUrlPath::Path(path) => Some(path),
        FileUrlPath::Invalid => None,
        FileUrlPath::NotFileUrl => {
            let bytes = reference.as_bytes();
            let windows = bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'/' | b'\\');
            (reference.starts_with('/') || reference.starts_with("\\\\") || windows)
                .then(|| PathBuf::from(reference))
        }
    }
}
impl PresetAssetReader for WebPresetAssets {
    fn read(&self, path: &Path) -> Result<Vec<u8>, String> {
        self.bytes(path)
    }
    fn exists(&self, path: &Path) -> bool {
        self.has(path)
    }
    fn is_local(&self, path: &Path) -> bool {
        image_source(&path.to_string_lossy()).is_some()
    }
    fn image_source(&self, reference: &str) -> Option<PathBuf> {
        image_source(reference)
    }
}
impl PresetAssetWriter for WebPresetAssets {
    type Error = String;
    fn create_dir_all(&self, path: &Path) -> Result<(), String> {
        match path.to_str() {
            Some("/assets/images" | "/assets/fonts" | "/assets/sounds") => Ok(()),
            _ => Err("invalid-web-asset-directory".into()),
        }
    }
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        let key = path.to_string_lossy();
        if !is_web_asset_key(&key) {
            return Err("invalid-web-asset-key".into());
        }
        self.writes
            .borrow_mut()
            .insert(key.into_owned(), bytes.to_vec());
        Ok(())
    }
    fn exists(&self, path: &Path) -> bool {
        self.has(path)
    }
    fn is_local(&self, path: &Path) -> bool {
        image_source(&path.to_string_lossy()).is_some()
    }
    fn image_source(&self, reference: &str) -> Option<PathBuf> {
        image_source(reference)
    }
    fn import_image_bytes(
        &self,
        bytes: &[u8],
        directory: &Path,
        extension: &str,
    ) -> Result<PathBuf, String> {
        let path = directory.join(format!("{}.{}", uuid::Uuid::new_v4(), extension));
        self.write(&path, bytes)?;
        Ok(path)
    }
    fn import_image_file(
        &self,
        source: &Path,
        directory: &Path,
        extension: &str,
    ) -> Result<PathBuf, String> {
        self.import_image_bytes(&self.bytes(source)?, directory, extension)
    }
    fn fill_missing_sprite_image_metrics(&self, positions: &mut SpritePositions) {
        for sprite in positions.values_mut().flatten() {
            if sprite.reference_natural_size.is_none() {
                if let Some((source, (width, height))) = sprite
                    .base_image
                    .as_deref()
                    .filter(|source| is_renderable_image_ref(Some(source)))
                    .and_then(|source| self.raster_size(source).map(|size| (source, size)))
                {
                    sprite.reference_natural_size = Some(SpriteReferenceNaturalSize {
                        source: Some(source.into()),
                        width,
                        height,
                    });
                }
            }
            for pose in &mut sprite.poses {
                if pose.image_override_metrics.is_none() {
                    if let Some((source, (width, height))) = pose
                        .image_override
                        .as_deref()
                        .filter(|source| is_renderable_image_ref(Some(source)))
                        .and_then(|source| self.raster_size(source).map(|size| (source, size)))
                    {
                        pose.image_override_metrics = Some(SpriteImageMetrics {
                            source: source.into(),
                            width,
                            height,
                        });
                    }
                }
            }
        }
    }
}
impl PresetImportHost for WebPresetAssets {
    type Error = String;
    fn invalid_preset(message: String) -> String {
        message
    }
    fn normalize_custom_css(&self, css: &mut CustomCss, _operation: &str) {
        if css
            .path
            .as_deref()
            .is_some_and(|path| !self.valid_css_path(path))
        {
            css.path = None;
        }
    }
    fn normalize_tab_css(&self, css: &mut TabCss, _operation: &str) {
        if css
            .path
            .as_deref()
            .is_some_and(|path| !self.valid_css_path(path))
        {
            css.path = None;
        }
    }
    fn restore_preset_local_fonts(
        &self,
        fonts: &mut FontSettings,
        embedded: Option<&[EmbeddedLocalFont]>,
    ) -> Result<(), String> {
        assets::restore_preset_local_fonts_in_dir(self, Path::new("/assets/fonts"), fonts, embedded)
    }
    fn restore_preset_local_images(
        &self,
        keys: &mut KeyPositions,
        stats: &mut StatPositions,
        graphs: &mut GraphPositions,
        knobs: &mut KnobPositions,
        sprites: &mut SpritePositions,
        embedded: Option<&[EmbeddedLocalImage]>,
    ) -> Result<(), String> {
        assets::restore_preset_local_images_in_dir(
            self,
            Path::new("/assets/images"),
            keys,
            stats,
            graphs,
            knobs,
            sprites,
            embedded,
        )
    }
    fn restore_preset_local_sounds(
        &self,
        keys: &mut KeyPositions,
        stats: &mut StatPositions,
        graphs: &mut GraphPositions,
        knobs: &mut KnobPositions,
        embedded: Option<&[EmbeddedLocalSound]>,
    ) -> Result<(), String> {
        assets::restore_preset_local_sounds_in_dir(
            self,
            Path::new("/assets/sounds"),
            keys,
            stats,
            graphs,
            knobs,
            embedded,
        )
    }
}

pub fn prepare_import(
    store: &AppStoreData,
    preset: PresetFile,
    tab_only: bool,
    assets: &WebAssetMap,
) -> Result<WebPresetImport, String> {
    let assets = WebPresetAssets::new(assets)?;
    let mut candidate = store.clone();
    let (settings_diff, publication) = if tab_only {
        let applied = plan::prepare_tab_preset(
            preset,
            store.selected_key_type.clone(),
            &store.font_settings,
            &assets,
        )?
        .apply(&mut candidate);
        (applied.0, None)
    } else {
        let plan = plan::prepare_full_preset(preset, store, &assets)?;
        let publication = plan.publication.clone();
        let applied = plan
            .apply(&mut candidate)
            .map_err(|error| error.to_string())?;
        (Some(applied.0), Some(publication))
    };
    Ok(WebPresetImport {
        store: candidate,
        asset_writes: assets.into_writes(),
        settings_diff,
        publication,
    })
}
pub fn export_preset(
    store: &AppStoreData,
    tab_only: bool,
    assets: &WebAssetMap,
) -> Result<PresetFile, String> {
    let assets = WebPresetAssets::new(assets)?;
    if tab_only {
        plan::build_tab_preset(store.clone(), &assets)
    } else {
        plan::build_full_preset(store.clone(), &assets)
    }
}

#[cfg(test)]
mod tests;

/// 기본 배치 복원 뒤 인라인 그림만 논리 자산으로 변환. 기존 누락 참조는 유지한다.
pub fn restore_inline_store_images(
    store: &mut AppStoreData,
    assets: &WebAssetMap,
) -> Result<WebAssetMap, String> {
    let memory = WebPresetAssets::new(assets)?;
    let migrate = |reference: &mut Option<String>| -> Result<(), String> {
        if let Some((bytes, extension)) = reference
            .as_deref()
            .and_then(|value| crate::preset::decode_image_data_url(value.trim()))
        {
            let path =
                memory.import_image_bytes(&bytes, Path::new("/assets/images"), &extension)?;
            *reference = Some(path.to_string_lossy().into_owned());
        }
        Ok(())
    };
    for position in store
        .key_positions
        .values_mut()
        .flatten()
        .chain(
            store
                .stat_positions
                .values_mut()
                .flatten()
                .map(|item| &mut item.position),
        )
        .chain(
            store
                .graph_positions
                .values_mut()
                .flatten()
                .map(|item| &mut item.position),
        )
        .chain(
            store
                .knob_positions
                .values_mut()
                .flatten()
                .map(|item| &mut item.position),
        )
    {
        migrate(&mut position.active_image)?;
        migrate(&mut position.inactive_image)?;
    }
    for sprite in store.sprite_positions.values_mut().flatten() {
        rewrite_coupled_sprite_image_reference(sprite, migrate)?;
        for pose in &mut sprite.poses {
            rewrite_coupled_sprite_image_reference(pose, migrate)?;
        }
    }
    Ok(memory.into_writes())
}
