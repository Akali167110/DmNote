//! 프리셋 데이터 준비와 적용. 파일·CSS 권한·저장 트랜잭션은 호스트가 소유한다.
mod export;
mod import;
use super::{EmbeddedLocalFont, EmbeddedLocalImage, EmbeddedLocalSound};
use crate::models::*;
pub use export::{build_full_preset, build_tab_preset};
pub use import::{
    prepare_full_preset, prepare_tab_preset, FullPresetApplyResult, FullPresetPlan,
    TabPresetApplyResult, TabPresetPlan,
};
#[derive(Clone, Debug)]
pub struct ImportedCssPaths {
    pub global: Option<String>,
    pub tabs: Vec<String>,
}
/// 기존 프리셋 이벤트는 준비 시점의 CSS·JS payload를 발행한다.
#[derive(Clone, Debug)]
pub struct FullPresetPublication {
    pub use_custom_css: bool,
    pub custom_css: CustomCss,
    pub use_custom_js: bool,
    pub custom_js: CustomJs,
}
/// 모든 콜백은 준비 단계에서 수행. 웹 호스트는 자산을 메모리에 준비한 후 저장한다.
pub trait PresetImportHost {
    type Error;
    fn invalid_preset(message: String) -> Self::Error;
    fn normalize_custom_css(&self, css: &mut CustomCss, operation: &str);
    fn normalize_tab_css(&self, css: &mut TabCss, operation: &str);
    fn restore_preset_local_fonts(
        &self,
        fonts: &mut FontSettings,
        embedded: Option<&[EmbeddedLocalFont]>,
    ) -> Result<(), Self::Error>;
    fn restore_preset_local_images(
        &self,
        keys: &mut KeyPositions,
        stats: &mut StatPositions,
        graphs: &mut GraphPositions,
        knobs: &mut KnobPositions,
        sprites: &mut SpritePositions,
        embedded: Option<&[EmbeddedLocalImage]>,
    ) -> Result<(), Self::Error>;
    fn restore_preset_local_sounds(
        &self,
        keys: &mut KeyPositions,
        stats: &mut StatPositions,
        graphs: &mut GraphPositions,
        knobs: &mut KnobPositions,
        embedded: Option<&[EmbeddedLocalSound]>,
    ) -> Result<(), Self::Error>;
}
#[cfg(test)]
mod tests;
