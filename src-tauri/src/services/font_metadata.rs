#[cfg(test)]
use crate::models::FontWeightRange;
use anyhow::{Context, Result};
pub use dmnote_editor_engine::web_resources::font::{parse_font_metadata_bytes, FontMetadata};
use std::path::Path;
pub fn parse_font_metadata(path: &Path) -> Result<FontMetadata> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read font metadata from {}", path.display()))?;
    parse_font_metadata_bytes(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bundled_variable_woff2_metadata() {
        let bytes = include_bytes!(
            "../../../packages/editor/src/renderer/assets/fonts/PretendardVariable.woff2"
        );
        let metadata = parse_font_metadata_bytes(bytes).unwrap();

        assert!(metadata
            .family_name
            .as_deref()
            .is_some_and(|name| name.contains("Pretendard")));
        assert_eq!(
            metadata.weight_ranges,
            vec![FontWeightRange { min: 45, max: 930 }]
        );
    }
}
