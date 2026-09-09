use super::*;
#[cfg(test)]
pub(super) use dmnote_editor_engine::preset::validation::invalid_position_style_detail;
pub(super) fn read_preset_file(path: &Path) -> CmdResult<PresetFile> {
    let content = fs::read_to_string(path)?;
    dmnote_editor_engine::preset::validation::decode_preset(&content).map_err(CommandError::msg)
}
#[cfg(test)]
pub(crate) fn read_preset_file_for_simulation(path: &Path) -> CmdResult<PresetFile> {
    read_preset_file(path)
}
