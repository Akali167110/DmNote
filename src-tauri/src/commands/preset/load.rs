#[cfg(test)]
use std::collections::HashMap;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use tauri::{AppHandle, Manager, WebviewWindow};

use crate::{
    commands::dialog::parented_file_dialog,
    commands::editor::{
        css::TabCssResponse,
        state::{emit_best_effort, publish_editor_change_after_key_runtime},
    },
    commands::{issue_mutation_ticket, run_blocking},
    custom_css::validate_css_path,
    errors::{CmdResult, CommandError},
    models::{
        AppStoreData, CustomCss, EditorCommitOrigin, EditorField, FontSettings, FontType,
        GraphPositions, KeyPositions, KnobPositions, SpritePositions, StatPositions, TabCss,
        TabCssOverrides,
    },
    state::{
        assets::image_asset::{import_image_bytes, import_image_file},
        migration::fill_missing_sprite_image_metrics,
        AppState,
    },
};

use super::{
    option_has_non_empty_text, EmbeddedLocalFont, EmbeddedLocalImage, EmbeddedLocalSound,
    PresetFile, PresetOperationResult,
};

mod imported_assets;
mod validation;

#[cfg(test)]
use imported_assets::{
    merge_tab_preset_fonts, restore_position_image_reference, restore_preset_local_fonts_in_dir,
    restore_preset_local_images_in_dir, restore_preset_local_sounds_in_dir,
};
use imported_assets::{
    restore_preset_local_fonts, restore_preset_local_images, restore_preset_local_sounds,
};
#[cfg(test)]
use validation::invalid_position_style_detail;
use validation::read_preset_file;
#[cfg(test)]
pub(crate) use validation::read_preset_file_for_simulation;

use dmnote_editor_engine::preset::plan::{
    prepare_full_preset, prepare_tab_preset, ImportedCssPaths, PresetImportHost,
};
struct NativePresetImportHost<'a>(&'a AppHandle);
impl PresetImportHost for NativePresetImportHost<'_> {
    type Error = CommandError;
    fn invalid_preset(message: String) -> Self::Error {
        CommandError::msg(message)
    }
    fn normalize_custom_css(&self, css: &mut CustomCss, operation: &str) {
        normalize_imported_custom_css(css, operation);
    }
    fn normalize_tab_css(&self, css: &mut TabCss, operation: &str) {
        normalize_imported_tab_css(css, operation);
    }
    fn restore_preset_local_fonts(
        &self,
        fonts: &mut FontSettings,
        embedded: Option<&[EmbeddedLocalFont]>,
    ) -> CmdResult<()> {
        restore_preset_local_fonts(self.0, fonts, embedded)
    }
    fn restore_preset_local_images(
        &self,
        keys: &mut KeyPositions,
        stats: &mut StatPositions,
        graphs: &mut GraphPositions,
        knobs: &mut KnobPositions,
        sprites: &mut SpritePositions,
        embedded: Option<&[EmbeddedLocalImage]>,
    ) -> CmdResult<()> {
        restore_preset_local_images(self.0, keys, stats, graphs, knobs, sprites, embedded)
    }
    fn restore_preset_local_sounds(
        &self,
        keys: &mut KeyPositions,
        stats: &mut StatPositions,
        graphs: &mut GraphPositions,
        knobs: &mut KnobPositions,
        embedded: Option<&[EmbeddedLocalSound]>,
    ) -> CmdResult<()> {
        restore_preset_local_sounds(self.0, keys, stats, graphs, knobs, embedded)
    }
}

#[tauri::command]
pub async fn preset_load(
    app: AppHandle,
    window: WebviewWindow,
) -> CmdResult<PresetOperationResult> {
    let picked = parented_file_dialog(&window, "DM NOTE Preset", &["json"])
        .pick_file()
        .await;

    let Some(file) = picked else {
        return Ok(PresetOperationResult {
            success: false,
            error: None,
        });
    };
    let path = file.path().to_path_buf();
    let window_label = window.label().to_string();
    run_blocking(app, move |app, state| {
        preset_load_from_path(app, state, &window_label, path)
    })
    .await
}

fn preset_load_from_path(
    app: &AppHandle,
    state: &AppState,
    window_label: &str,
    path: PathBuf,
) -> CmdResult<PresetOperationResult> {
    state.ensure_mutation_allowed().map_err(CommandError::msg)?;
    let preset = read_preset_file(&path)?;
    let current = state.store.snapshot();
    let plan = prepare_full_preset(preset, &current, &NativePresetImportHost(app))?;
    let imported_css_paths = plan.imported_css_paths.clone();
    let publication = plan.publication.clone();
    let ticket = issue_mutation_ticket(app)?;
    let admission = state.admit_frontend_history_mutation(window_label)?;
    ticket.run(move || {
        state.ensure_mutation_allowed().map_err(CommandError::msg)?;
        let css_operation_guard = state.lock_css_operation();
        let previous_css_state = state.store.snapshot();
        let (transaction, _) = state.commit_preset_editor_transaction_preserving_runtime_counters(
            app,
            EditorCommitOrigin::LegacyAdapter("preset_load".to_string()),
            &[
                EditorField::Keys,
                EditorField::KeyPositions,
                EditorField::StatPositions,
                EditorField::GraphPositions,
                EditorField::KnobPositions,
                EditorField::SpritePositions,
                EditorField::LayerGroups,
            ],
            admission,
            move |store| plan.apply(store),
        )?;
        if !transaction
            .change
            .result
            .changed_fields
            .contains(&EditorField::Keys)
        {
            state.apply_committed_editor_keys_without_counters(
                transaction.change.runtime_publication_generation,
                &transaction.change.document.keys,
                &transaction.change.selected_key_type,
            );
        }
        let current_css_state = state.store.snapshot();
        authorize_committed_preset_css_paths(state, &current_css_state, &imported_css_paths);
        state.resync_global_css_watcher(&previous_css_state, &current_css_state);
        sync_tab_css_runtime(state, app, &transaction.value.1, &transaction.value.6);
        drop(css_operation_guard);
        publish_editor_change_after_key_runtime(state, app, &transaction.change);
        state.obs_broadcast_counters();

        let history_status = transaction.change.history_status.clone();
        let (diff, _, custom_tabs, tab_order, bar_count, tab_note_overrides, _) = transaction.value;
        let selected_key_type = transaction.change.selected_key_type.clone();
        let keys = transaction.change.document.keys;
        let positions = transaction.change.document.key_positions;
        let stat_positions = transaction.change.document.stat_positions;
        let graph_positions = transaction.change.document.graph_positions;
        let knob_positions = transaction.change.document.knob_positions;
        let sprite_positions = transaction.change.document.sprite_positions;
        let layer_groups = transaction.change.document.layer_groups;

        if let Err(error) = state.emit_settings_changed(&diff, app) {
            log::error!("[Preset] failed to publish settings change: {error:#}");
        }
        emit_best_effort(app, "layerGroups:changed", &layer_groups);
        // 프리셋 데이터를 단일 이벤트로 원자적 전달
        emit_best_effort(
            app,
            "preset:snapshot",
            &super::PresetSnapshot {
                keys,
                positions,
                stat_positions,
                graph_positions,
                knob_positions,
                sprite_positions,
                custom_tabs,
                tab_order,
                bar_count,
                selected_key_type,
                tab_note_overrides,
            },
        );
        emit_best_effort(
            app,
            "css:use",
            &serde_json::json!({ "enabled": publication.use_custom_css }),
        );
        emit_best_effort(app, "css:content", &publication.custom_css);
        emit_best_effort(
            app,
            "js:use",
            &serde_json::json!({ "enabled": publication.use_custom_js }),
        );
        emit_best_effort(app, "js:content", &publication.custom_js);

        // OBS 브릿지: 프리셋 로드 시 전체 스냅샷 재전송
        state.refresh_obs_snapshot();
        if let Some(status) = history_status.as_ref() {
            emit_best_effort(app, "history:status", status);
        }
        Ok(PresetOperationResult {
            success: true,
            error: None,
        })
    })
}

#[tauri::command]
pub async fn preset_load_tab(
    app: AppHandle,
    window: WebviewWindow,
) -> CmdResult<PresetOperationResult> {
    let picked = parented_file_dialog(&window, "DM NOTE Preset", &["json"])
        .pick_file()
        .await;

    let Some(file) = picked else {
        return Ok(PresetOperationResult {
            success: false,
            error: None,
        });
    };
    let path = file.path().to_path_buf();
    let window_label = window.label().to_string();
    run_blocking(app, move |app, state| {
        preset_load_tab_from_path(app, state, &window_label, path)
    })
    .await
}

fn preset_load_tab_from_path(
    app: &AppHandle,
    state: &AppState,
    window_label: &str,
    path: PathBuf,
) -> CmdResult<PresetOperationResult> {
    state.ensure_mutation_allowed().map_err(CommandError::msg)?;
    let preset = read_preset_file(&path)?;

    let (current_tab_id, existing_font_settings) = state
        .store
        .with_state(|store| (store.selected_key_type.clone(), store.font_settings.clone()));

    let plan = prepare_tab_preset(
        preset,
        current_tab_id,
        &existing_font_settings,
        &NativePresetImportHost(app),
    )?;
    let imported_css_paths = plan.imported_css_paths.clone();

    let ticket = issue_mutation_ticket(app)?;
    let admission = state.admit_frontend_history_mutation(window_label)?;
    ticket.run(move || {
        state.ensure_mutation_allowed().map_err(CommandError::msg)?;
        let css_operation_guard = state.lock_css_operation();
        let (transaction, _) = state.commit_preset_editor_transaction_preserving_runtime_counters(
            app,
            EditorCommitOrigin::LegacyAdapter("preset_load_tab".to_string()),
            &[
                EditorField::Keys,
                EditorField::KeyPositions,
                EditorField::StatPositions,
                EditorField::GraphPositions,
                EditorField::KnobPositions,
                EditorField::SpritePositions,
                EditorField::LayerGroups,
            ],
            admission,
            move |store| Ok(plan.apply(store)),
        )?;
        if !transaction
            .change
            .result
            .changed_fields
            .contains(&EditorField::Keys)
        {
            state.apply_committed_editor_keys_without_counters(
                transaction.change.runtime_publication_generation,
                &transaction.change.document.keys,
                &transaction.change.selected_key_type,
            );
        }
        authorize_committed_preset_css_paths(state, &state.store.snapshot(), &imported_css_paths);
        sync_tab_css_runtime(state, app, &transaction.value.1, &transaction.value.3);
        drop(css_operation_guard);
        publish_editor_change_after_key_runtime(state, app, &transaction.change);
        state.obs_broadcast_counters();
        let history_status = transaction.change.history_status.clone();
        let (settings_diff, _, full_tab_note_overrides, _) = transaction.value;
        let full_keys = transaction.change.document.keys;
        let full_positions = transaction.change.document.key_positions;
        let full_stat_positions = transaction.change.document.stat_positions;
        let full_graph_positions = transaction.change.document.graph_positions;
        let full_knob_positions = transaction.change.document.knob_positions;
        let full_sprite_positions = transaction.change.document.sprite_positions;
        let full_layer_groups = transaction.change.document.layer_groups;

        if let Some(diff) = settings_diff.as_ref() {
            if let Err(error) = state.emit_settings_changed(diff, app) {
                log::error!("[Preset] failed to publish tab preset settings: {error:#}");
            }
        }

        emit_best_effort(app, "layerGroups:changed", &full_layer_groups);
        emit_best_effort(app, "keys:changed", &full_keys);
        emit_best_effort(app, "positions:changed", &full_positions);
        emit_best_effort(app, "statPositions:changed", &full_stat_positions);
        emit_best_effort(app, "graphPositions:changed", &full_graph_positions);
        emit_best_effort(app, "knobPositions:changed", &full_knob_positions);
        emit_best_effort(app, "spritePositions:changed", &full_sprite_positions);
        emit_best_effort(app, "tabNote:changed_all", &full_tab_note_overrides);

        // OBS 브릿지: 탭 프리셋 로드 시 전체 스냅샷 재전송
        state.refresh_obs_snapshot();
        if let Some(status) = history_status.as_ref() {
            emit_best_effort(app, "history:status", status);
        }
        Ok(PresetOperationResult {
            success: true,
            error: None,
        })
    })
}

fn sync_tab_css_runtime(
    state: &AppState,
    app: &AppHandle,
    previous: &TabCssOverrides,
    current: &TabCssOverrides,
) {
    let tab_ids: BTreeSet<String> = previous.keys().chain(current.keys()).cloned().collect();

    for tab_id in tab_ids {
        if previous.get(&tab_id) == current.get(&tab_id) {
            continue;
        }

        state.unwatch_tab_css(&tab_id);
        let css = current.get(&tab_id).cloned();
        if let Some(tab_css) = css.as_ref() {
            if tab_css.enabled {
                if let Some(path) = tab_css.path.as_deref() {
                    if let Err(error) = state.watch_tab_css(path, &tab_id) {
                        log::warn!("[Preset] 탭 CSS 감시 시작 실패 (tab={tab_id}): {error}");
                    }
                }
            }
        }

        emit_best_effort(
            app,
            "tabCss:changed",
            &TabCssResponse {
                tab_id: tab_id.clone(),
                css,
            },
        );
    }
}

fn authorize_committed_preset_css_paths(
    state: &AppState,
    committed: &AppStoreData,
    imported: &ImportedCssPaths,
) {
    for path in committed_preset_css_paths(committed, imported) {
        state.authorize_css_path(&path);
    }
}

fn committed_preset_css_paths(
    committed: &AppStoreData,
    imported: &ImportedCssPaths,
) -> Vec<String> {
    let mut paths = BTreeSet::new();
    if let Some(path) = imported
        .global
        .as_ref()
        .filter(|path| committed.custom_css.path.as_ref() == Some(path))
    {
        paths.insert(path.clone());
    }
    for path in &imported.tabs {
        if committed
            .tab_css_overrides
            .values()
            .any(|css| css.path.as_ref() == Some(path))
        {
            paths.insert(path.clone());
        }
    }
    paths.into_iter().collect()
}

fn normalize_imported_custom_css(css: &mut CustomCss, operation: &str) {
    let Some(path) = css.path.clone() else {
        return;
    };
    match validate_css_path(Path::new(&path)) {
        Ok(loaded) => css.path = Some(loaded.canonical_path),
        Err(error) => {
            log::warn!(
                "[{operation}] Dropped invalid global CSS path code={} path={} detail={}",
                error.code.as_str(),
                path,
                error.detail
            );
            css.path = None;
        }
    }
}

fn normalize_imported_tab_css(css: &mut TabCss, operation: &str) {
    let Some(path) = css.path.clone() else {
        return;
    };
    match validate_css_path(Path::new(&path)) {
        Ok(loaded) => css.path = Some(loaded.canonical_path),
        Err(error) => {
            log::warn!(
                "[{operation}] Dropped invalid tab CSS path code={} path={} detail={}",
                error.code.as_str(),
                path,
                error.detail
            );
            css.path = None;
        }
    }
}

/// 탭 프리셋의 요소 컬렉션 하나를 대상 탭 키로 옮긴다. 컬렉션이 있는데 원본 탭 항목이
/// 없으면 "요소 없음"이라 빈 목록을 넣어 기존 요소를 지운다 - 없으면 남는다.
/// 컬렉션 자체가 없는 옛 프리셋만 기존 요소를 그대로 둔다
pub use dmnote_editor_engine::preset::import::*;
#[cfg(test)]
mod tests;
