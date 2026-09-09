use crate::commit::*;
use crate::{
    errors::EditorCommitError,
    models::*,
    state::{editor::*, history::*, plugin::*},
};
use std::collections::HashSet;
#[derive(Debug)]
pub struct HistoryOperationResult {
    pub status: HistoryStatus,
    pub change: Option<CommittedEditorChange>,
    pub aux_change: Option<HistoryAuxChange>,
    pub replayed: bool,
    pub runtime_publication_generation: u64,
}

#[derive(Debug)]
pub enum HistoryAuxChange {
    CustomTabs {
        snapshot: Box<CustomTabsHistorySnapshot>,
        changed_tab_css_ids: Vec<String>,
        plugin_ids: Vec<String>,
        revision: u64,
    },
    PresetFull {
        snapshot: Box<PresetFullHistorySnapshot>,
        settings_diff: Box<SettingsDiff>,
        changed_tab_css_ids: Vec<String>,
    },
    Mode(String),
    Counters(KeyCounters),
    PluginElements {
        plugin_id: String,
        revision: u64,
    },
    PluginElementsBatch {
        plugin_ids: Vec<String>,
        revision: u64,
    },
}

pub fn preset_history_settings_patch(
    settings: &PresetHistorySettingsSnapshot,
) -> SettingsPatchInput {
    let note = &settings.note_settings;
    SettingsPatchInput {
        note_effect: Some(settings.note_effect),
        laboratory_enabled: Some(settings.laboratory_enabled),
        note_settings: Some(NoteSettingsPatch {
            frame_limit: Some(note.frame_limit),
            speed: Some(note.speed),
            track_height: Some(note.track_height),
            reverse: Some(note.reverse),
            fade_position: Some(note.fade_position.clone()),
            fade_top_px: Some(note.fade_top_px),
            fade_bottom_px: Some(note.fade_bottom_px),
            reverse_fade_top_px: Some(note.reverse_fade_top_px),
            reverse_fade_bottom_px: Some(note.reverse_fade_bottom_px),
            delayed_note_enabled: Some(note.delayed_note_enabled),
            short_note_threshold_ms: Some(note.short_note_threshold_ms),
            short_note_min_length_px: Some(note.short_note_min_length_px),
            key_display_delay_ms: Some(note.key_display_delay_ms),
        }),
        background_color: Some(settings.background_color.clone()),
        use_custom_css: Some(settings.use_custom_css),
        custom_css: Some(CustomCssPatch {
            path: Some(settings.custom_css.path.clone()),
            content: Some(settings.custom_css.content.clone()),
        }),
        font_settings: Some(settings.font_settings.clone()),
        use_custom_js: Some(settings.use_custom_js),
        custom_js: Some(CustomJsPatch {
            path: Some(settings.custom_js.path.clone()),
            content: Some(settings.custom_js.content.clone()),
            plugins: Some(settings.custom_js.plugins.clone()),
        }),
        ..SettingsPatchInput::default()
    }
}
pub fn changed_map_ids<T: PartialEq>(
    current: &std::collections::HashMap<String, T>,
    target: &std::collections::HashMap<String, T>,
) -> Vec<String> {
    let mut ids = current
        .keys()
        .chain(target.keys())
        .filter(|id| current.get(*id) != target.get(*id))
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    ids.sort();
    ids
}
pub fn require_history_entry(plan: HistoryRecordPlan) -> Result<HistoryEntry, String> {
    match plan {
        HistoryRecordPlan::Entry(entry) => Ok(*entry),
        HistoryRecordPlan::Merge { .. } => Err(HISTORY_INVALID_OPPOSITE_ENTRY.to_string()),
        HistoryRecordPlan::Truncate => Err(HISTORY_ENTRY_TOO_LARGE.to_string()),
    }
}
pub fn editor_history_error(error: EditorCommitError) -> String {
    format!("{:?}: {}", error.error_code, error.message)
}
pub fn project_history_key_counters(
    current: &KeyCounters,
    historical: &KeyCounters,
    target_keys: &KeyMappings,
) -> KeyCounters {
    let mut projected = historical.clone();
    sync_key_counters(&mut projected, target_keys);
    for (mode, counters) in &mut projected {
        let Some(current_mode) = current.get(mode) else {
            continue;
        };
        for (key, count) in counters {
            if let Some(current_count) = current_mode.get(key) {
                *count = *current_count;
            }
        }
    }
    projected
}
pub fn project_editor_history_key_counters(
    current: &KeyCounters,
    historical: Option<&KeyCounters>,
    target_keys: &KeyMappings,
) -> KeyCounters {
    let Some(historical) = historical else {
        let mut projected = current.clone();
        sync_key_counters(&mut projected, target_keys);
        return projected;
    };
    project_history_key_counters(current, historical, target_keys)
}

struct HistoryRestorer;
struct HistoryDraft {
    store: AppStoreData,
    history: HistoryService,
    plugin_revision: u64,
}

impl HistoryRestorer {
    pub fn apply_history_operation(
        &self,
        direction: HistoryDirection,
        operation_id: &str,
        current_key_counters: &KeyCounters,
        guard: &mut HistoryDraft,
    ) -> Result<HistoryOperationResult, String> {
        if operation_id.len() > 64 || uuid::Uuid::parse_str(operation_id).is_err() {
            return Err(INVALID_HISTORY_OPERATION_ID.to_string());
        }

        if guard.history.operation_replayed(operation_id, direction)? {
            return Ok(HistoryOperationResult {
                status: guard.history.status(false),
                change: None,
                aux_change: None,
                replayed: true,
                runtime_publication_generation: 0,
            });
        }
        guard.history.begin_barrier();

        let result = (|| {
            let target = guard
                .history
                .target(direction)
                .cloned()
                .ok_or_else(|| direction.empty_error().to_string())?;
            if target.scope != target.before.scope() {
                return Err(HISTORY_SCOPE_MISMATCH.to_string());
            }
            let target_gesture_id = target.gesture_id.clone();
            let target_gesture_ids = target.gesture_ids.clone();

            let current_store = guard.store.clone();
            let origin = match direction {
                HistoryDirection::Undo => EditorCommitOrigin::HistoryUndo,
                HistoryDirection::Redo => EditorCommitOrigin::HistoryRedo,
            };
            let (opposite, change, aux_change) = match target.before {
                HistorySnapshot::Editor {
                    changed_fields,
                    before,
                    key_counters,
                } => {
                    let restores_keys = changed_fields.contains(&EditorField::Keys);
                    let current = EditorDocumentV1::from_store(&current_store);
                    let opposite =
                        require_history_entry(guard.history.prepare_opposite_editor_entry(
                            changed_fields.clone(),
                            current.patch_for_fields(&changed_fields),
                            restores_keys.then(|| current_key_counters.clone()),
                            target_gesture_id,
                        )?)?;
                    let projected_counters = if restores_keys {
                        let target_keys = before.keys.as_ref().ok_or_else(|| {
                            "history key snapshot is missing target mappings".to_string()
                        })?;
                        Some(project_editor_history_key_counters(
                            current_key_counters,
                            key_counters.as_ref(),
                            target_keys,
                        ))
                    } else {
                        None
                    };
                    let change = self
                        .commit_editor_patch_locked(
                            guard,
                            &before,
                            &changed_fields,
                            projected_counters.as_ref(),
                            EditorPatchCommitOptions {
                                mutation_id: operation_id.to_string(),
                                gesture_id: None,
                                gesture_ids: Vec::new(),
                                origin,
                                record_history: false,
                                apply_key_side_effects: restores_keys,
                                enforce_touched_fields: true,
                            },
                        )
                        .map_err(editor_history_error)?;
                    if change.result.changed_fields.is_empty() {
                        return Err(HISTORY_TARGET_ALREADY_APPLIED.to_string());
                    }
                    (opposite, Some(change), None)
                }
                HistorySnapshot::CustomTabs(before) => {
                    let mut current_snapshot =
                        CustomTabsHistorySnapshot::from_store_for_target(&current_store, &before);
                    current_snapshot.key_counters = current_key_counters.clone();
                    let mut changed_tab_css_ids =
                        before.tab_css_patch.keys().cloned().collect::<Vec<_>>();
                    changed_tab_css_ids.sort();
                    let opposite = require_history_entry(
                        guard.history.prepare_custom_tabs_entry(current_snapshot)?,
                    )?;
                    let (change, plugin_ids, revision) = self
                        .commit_custom_tabs_history_locked(
                            guard,
                            &before,
                            operation_id,
                            origin,
                            current_key_counters,
                        )
                        .map_err(editor_history_error)?;
                    (
                        opposite,
                        change,
                        Some(HistoryAuxChange::CustomTabs {
                            snapshot: before,
                            changed_tab_css_ids,
                            plugin_ids,
                            revision,
                        }),
                    )
                }
                HistorySnapshot::PresetFull(before) => {
                    if before.matches_store(&current_store) {
                        return Err(HISTORY_TARGET_ALREADY_APPLIED.to_string());
                    }
                    let mut current_snapshot =
                        PresetFullHistorySnapshot::from_store(&current_store);
                    current_snapshot.key_counters = current_key_counters.clone();
                    let changed_tab_css_ids = changed_map_ids(
                        &current_store.tab_css_overrides,
                        &before.tab_css_overrides,
                    );
                    let opposite = require_history_entry(
                        guard.history.prepare_preset_full_entry(current_snapshot)?,
                    )?;
                    let (change, settings_diff) = self
                        .commit_preset_full_history_locked(
                            guard,
                            &before,
                            operation_id,
                            origin,
                            current_key_counters,
                        )
                        .map_err(editor_history_error)?;
                    (
                        opposite,
                        change,
                        Some(HistoryAuxChange::PresetFull {
                            snapshot: before,
                            settings_diff: Box::new(settings_diff),
                            changed_tab_css_ids,
                        }),
                    )
                }
                HistorySnapshot::Mode(before) => {
                    let opposite = require_history_entry(
                        guard
                            .history
                            .prepare_mode_entry(current_store.selected_key_type.clone())?,
                    )?;
                    if current_store.selected_key_type == before {
                        return Err(HISTORY_TARGET_ALREADY_APPLIED.to_string());
                    }
                    let mut scratch = current_store;
                    scratch.selected_key_type = before.clone();
                    self.commit_locked(guard, scratch, ())
                        .map_err(|error| error.to_string())?;
                    (opposite, None, Some(HistoryAuxChange::Mode(before)))
                }
                HistorySnapshot::Counters(before) => {
                    let opposite = require_history_entry(
                        guard
                            .history
                            .prepare_counters_entry(current_key_counters.clone())?,
                    )?;
                    if current_store.key_counters == before {
                        return Err(HISTORY_TARGET_ALREADY_APPLIED.to_string());
                    }
                    let mut scratch = current_store;
                    scratch.key_counters = before.clone();
                    self.commit_locked(guard, scratch, ())
                        .map_err(|error| error.to_string())?;
                    (opposite, None, Some(HistoryAuxChange::Counters(before)))
                }
                HistorySnapshot::PluginElements(before) => {
                    let current_snapshot =
                        plugin_elements_snapshot(&current_store, &before.plugin_id)?;
                    if current_snapshot == before {
                        return Err(HISTORY_TARGET_ALREADY_APPLIED.to_string());
                    }
                    let opposite = require_history_entry(
                        guard
                            .history
                            .prepare_opposite_plugin_elements_entry(current_snapshot)?,
                    )?;
                    let mut scratch = current_store;
                    apply_plugin_elements_snapshot(&mut scratch, &before)?;
                    let plugin_model_revision = next_plugin_model_revision(guard.plugin_revision)?;
                    self.commit_locked(guard, scratch, ())
                        .map_err(|error| error.to_string())?;
                    guard.plugin_revision = plugin_model_revision;
                    (
                        opposite,
                        None,
                        Some(HistoryAuxChange::PluginElements {
                            plugin_id: before.plugin_id,
                            revision: plugin_model_revision,
                        }),
                    )
                }
                HistorySnapshot::Compound { snapshots } => {
                    let current = EditorDocumentV1::from_store(&current_store);
                    let mut opposite_snapshots = Vec::with_capacity(snapshots.len());
                    for snapshot in &snapshots {
                        match snapshot {
                            HistorySnapshot::Editor { changed_fields, .. } => {
                                opposite_snapshots.push(HistorySnapshot::Editor {
                                    changed_fields: changed_fields.clone(),
                                    before: Box::new(current.patch_for_fields(changed_fields)),
                                    key_counters: changed_fields
                                        .contains(&EditorField::Keys)
                                        .then(|| current_key_counters.clone()),
                                });
                            }
                            HistorySnapshot::PluginElements(before) => {
                                opposite_snapshots.push(HistorySnapshot::PluginElements(
                                    plugin_elements_snapshot(&current_store, &before.plugin_id)?,
                                ));
                            }
                            _ => {
                                return Err(
                                    "compound history contains an unsupported snapshot".to_string()
                                )
                            }
                        }
                    }
                    let opposite =
                        require_history_entry(guard.history.prepare_opposite_compound_entry(
                            opposite_snapshots,
                            target_gesture_ids,
                        )?)?;
                    let (change, plugin_ids, plugin_model_revision) = self
                        .commit_compound_history_locked(
                            guard,
                            &snapshots,
                            operation_id,
                            origin,
                            current_key_counters,
                        )
                        .map_err(editor_history_error)?;
                    (
                        opposite,
                        change,
                        Some(HistoryAuxChange::PluginElementsBatch {
                            plugin_ids,
                            revision: plugin_model_revision,
                        }),
                    )
                }
            };

            guard
                .history
                .commit_operation(direction, operation_id.to_string(), opposite);
            Ok(HistoryOperationResult {
                status: guard.history.status(false),
                change,
                aux_change,
                replayed: false,
                runtime_publication_generation: 0,
            })
        })();
        guard.history.finish_barrier();
        result.map(|mut outcome| {
            outcome.status = guard.history.status(false);
            outcome
        })
    }

    fn commit_compound_history_locked(
        &self,
        guard: &mut HistoryDraft,
        snapshots: &[HistorySnapshot],
        operation_id: &str,
        origin: EditorCommitOrigin,
        current_key_counters: &KeyCounters,
    ) -> std::result::Result<(Option<CommittedEditorChange>, Vec<String>, u64), EditorCommitError>
    {
        let mut current_store = guard.store.clone();
        let mut editor_target = None;
        let mut plugin_targets = Vec::new();
        let mut seen_plugin_ids = HashSet::new();
        let mut restores_keys = false;

        for snapshot in snapshots {
            match snapshot {
                HistorySnapshot::Editor {
                    changed_fields,
                    before,
                    key_counters,
                } => {
                    if editor_target.is_some() {
                        return Err(EditorCommitError::validation(
                            "HISTORY_COMPOUND_INVALID",
                            "compound history contains duplicate editor snapshots",
                        ));
                    }
                    restores_keys = changed_fields.contains(&EditorField::Keys);
                    editor_target = Some((changed_fields, before, key_counters));
                }
                HistorySnapshot::PluginElements(target) => {
                    if !seen_plugin_ids.insert(target.plugin_id.as_str()) {
                        return Err(EditorCommitError::validation(
                            "HISTORY_COMPOUND_INVALID",
                            "compound history contains duplicate plugin snapshots",
                        ));
                    }
                    plugin_targets.push(target);
                }
                _ => {
                    return Err(EditorCommitError::validation(
                        "HISTORY_COMPOUND_INVALID",
                        "compound history contains an unsupported snapshot",
                    ))
                }
            }
        }

        if restores_keys {
            current_store.key_counters = current_key_counters.clone();
        }

        let (mut scratch, editor_restore) =
            if let Some((changed_fields, before, key_counters)) = editor_target {
                let (_, candidate, next_store, actual_fields) =
                    prepare_editor_patch_transition(&current_store, before, changed_fields)?;
                if actual_fields
                    .iter()
                    .any(|field| !changed_fields.contains(field))
                {
                    return Err(EditorCommitError::validation(
                        "HISTORY_RESTORE_CHANGED_UNDECLARED_FIELD",
                        "history restore changed an editor field outside its entry",
                    ));
                }
                (
                    next_store,
                    Some((candidate, actual_fields, key_counters.as_ref())),
                )
            } else {
                (current_store.clone(), None)
            };
        let mut plugin_ids = Vec::new();
        for target in plugin_targets {
            let current =
                plugin_elements_snapshot(&current_store, &target.plugin_id).map_err(|error| {
                    EditorCommitError::validation("HISTORY_COMPOUND_INVALID", error)
                })?;
            if current == *target {
                continue;
            }
            apply_plugin_elements_snapshot(&mut scratch, target).map_err(|error| {
                EditorCommitError::validation("HISTORY_COMPOUND_INVALID", error)
            })?;
            plugin_ids.push(target.plugin_id.clone());
        }

        let editor_changed = editor_restore
            .as_ref()
            .is_some_and(|(_, changed_fields, _)| !changed_fields.is_empty());
        if !editor_changed && plugin_ids.is_empty() {
            return Err(EditorCommitError::validation(
                "HISTORY_TARGET_ALREADY_APPLIED",
                "history target is already applied",
            ));
        }

        if let Some((candidate, changed_fields, historical_counters)) = editor_restore.as_ref() {
            if changed_fields.contains(&EditorField::Keys) {
                scratch.key_counters = project_editor_history_key_counters(
                    current_key_counters,
                    *historical_counters,
                    &candidate.keys,
                );
                repair_selected_mode(&mut scratch);
            }
        }

        let editor_revision = if editor_changed {
            let revision = next_revision(current_store.editor_revision)?;
            scratch.editor_revision = revision;
            revision
        } else {
            current_store.editor_revision
        };
        let plugin_model_revision = if plugin_ids.is_empty() {
            guard.plugin_revision
        } else {
            next_plugin_model_revision(guard.plugin_revision).map_err(|error| {
                EditorCommitError::validation("PLUGIN_MODEL_REVISION_OUT_OF_RANGE", error)
            })?
        };
        let selected_key_type = scratch.selected_key_type.clone();
        let key_counters = scratch.key_counters.clone();

        self.commit_locked(guard, scratch, ())
            .map_err(|error| EditorCommitError::io(error.to_string()))?;
        guard.plugin_revision = plugin_model_revision;

        let change = editor_restore.and_then(|(candidate, changed_fields, _)| {
            if changed_fields.is_empty() {
                return None;
            }
            let event = origin.event_name().map(|origin| EditorCommittedV1 {
                schema_version: EDITOR_SCHEMA_VERSION,
                revision: editor_revision,
                mutation_id: operation_id.to_string(),
                gesture_id: None,
                gesture_ids: Vec::new(),
                origin,
                changed_fields: changed_fields.clone(),
                patch: candidate.patch_for_fields(&changed_fields),
            });
            Some(CommittedEditorChange {
                result: EditorCommitResult {
                    revision: editor_revision,
                    changed_fields,
                    op_results: None,
                },
                event,
                replayed: false,
                document: candidate,
                selected_key_type,
                key_counters,
                history_status: None,
                plugin_instances_changes: Vec::new(),
                runtime_publication_generation: 0,
            })
        });

        Ok((change, plugin_ids, plugin_model_revision))
    }

    fn commit_custom_tabs_history_locked(
        &self,
        guard: &mut HistoryDraft,
        target: &CustomTabsHistorySnapshot,
        operation_id: &str,
        origin: EditorCommitOrigin,
        current_key_counters: &KeyCounters,
    ) -> std::result::Result<(Option<CommittedEditorChange>, Vec<String>, u64), EditorCommitError>
    {
        let current_store = guard.store.clone();
        if target.matches_store(&current_store) {
            return Err(EditorCommitError::validation(
                "HISTORY_TARGET_ALREADY_APPLIED",
                "history target is already applied",
            ));
        }

        validate_history_restore_metadata(
            &target.document,
            &target.custom_tabs,
            &target.tab_order,
            &target.selected_key_type,
        )?;
        let current = EditorDocumentV1::from_store(&current_store);
        let mut scratch = current_store.clone();
        target.document.apply_to_store(&mut scratch);
        scratch.custom_tabs = target.custom_tabs.clone();
        scratch.tab_order = target.tab_order.clone();
        scratch.bar_count =
            crate::state::tab_metadata::normalize_bar_count(target.bar_count, &scratch.tab_order);
        scratch.selected_key_type = target.selected_key_type.clone();
        scratch.key_counters = project_history_key_counters(
            current_key_counters,
            &target.key_counters,
            &target.document.keys,
        );
        target.apply_override_patches(&mut scratch);
        crate::state::migration::canonicalize_gradient_pairs(&mut scratch);
        crate::state::migration::canonicalize_image_modes(&mut scratch);
        crate::state::migration::normalize_sprite_triggers(&mut scratch);
        let candidate = EditorDocumentV1::from_store(&scratch);
        validate_paired_update(&current, &candidate, true, true)?;
        scratch.editor_revision = current_store.editor_revision;
        validate_document_transition(&current, &candidate, &current_store, &scratch)?;

        let changed_fields = current.changed_fields(&candidate);
        let revision = if changed_fields.is_empty() {
            current_store.editor_revision
        } else {
            let revision = next_revision(current_store.editor_revision)?;
            scratch.editor_revision = revision;
            revision
        };
        let selected_key_type = scratch.selected_key_type.clone();
        let key_counters = scratch.key_counters.clone();
        let plugin_ids = target
            .changed_plugin_ids()
            .into_iter()
            .filter(|plugin_id| {
                let key = plugin_instances_storage_key(plugin_id);
                current_store.plugin_data.get(&key) != scratch.plugin_data.get(&key)
            })
            .collect::<Vec<_>>();
        let plugin_model_revision = if plugin_ids.is_empty() {
            guard.plugin_revision
        } else {
            next_plugin_model_revision(guard.plugin_revision).map_err(|error| {
                EditorCommitError::validation("PLUGIN_MODEL_REVISION_OUT_OF_RANGE", error)
            })?
        };
        self.commit_locked(guard, scratch, ())
            .map_err(|error| EditorCommitError::io(error.to_string()))?;
        guard.plugin_revision = plugin_model_revision;

        if changed_fields.is_empty() {
            return Ok((None, plugin_ids, plugin_model_revision));
        }
        let event = origin.event_name().map(|origin| EditorCommittedV1 {
            schema_version: EDITOR_SCHEMA_VERSION,
            revision,
            mutation_id: operation_id.to_string(),
            gesture_id: None,
            gesture_ids: Vec::new(),
            origin,
            changed_fields: changed_fields.clone(),
            patch: candidate.patch_for_fields(&changed_fields),
        });
        Ok((
            Some(CommittedEditorChange {
                result: EditorCommitResult {
                    revision,
                    changed_fields,
                    op_results: None,
                },
                event,
                replayed: false,
                document: candidate,
                selected_key_type,
                key_counters,
                history_status: None,
                plugin_instances_changes: Vec::new(),
                runtime_publication_generation: 0,
            }),
            plugin_ids,
            plugin_model_revision,
        ))
    }

    fn commit_preset_full_history_locked(
        &self,
        guard: &mut HistoryDraft,
        target: &PresetFullHistorySnapshot,
        operation_id: &str,
        origin: EditorCommitOrigin,
        current_key_counters: &KeyCounters,
    ) -> std::result::Result<(Option<CommittedEditorChange>, SettingsDiff), EditorCommitError> {
        let current_store = guard.store.clone();
        validate_history_restore_metadata(
            &target.document,
            &target.custom_tabs,
            &target.tab_order,
            &target.selected_key_type,
        )?;
        let current = EditorDocumentV1::from_store(&current_store);
        let mut scratch = current_store.clone();
        target.document.apply_to_store(&mut scratch);
        scratch.custom_tabs = target.custom_tabs.clone();
        scratch.tab_order = target.tab_order.clone();
        scratch.bar_count =
            crate::state::tab_metadata::normalize_bar_count(target.bar_count, &scratch.tab_order);
        scratch.selected_key_type = target.selected_key_type.clone();
        scratch.key_counters = project_history_key_counters(
            current_key_counters,
            &target.key_counters,
            &target.document.keys,
        );
        scratch.tab_note_overrides = target.settings.tab_note_overrides.clone();
        scratch.tab_css_overrides = target.tab_css_overrides.clone();
        let settings_diff = crate::settings::apply_patch_to_store(
            &mut scratch,
            &preset_history_settings_patch(&target.settings),
        );
        crate::state::migration::canonicalize_gradient_pairs(&mut scratch);
        crate::state::migration::canonicalize_image_modes(&mut scratch);
        crate::state::migration::normalize_sprite_triggers(&mut scratch);
        let candidate = EditorDocumentV1::from_store(&scratch);
        validate_paired_update(&current, &candidate, true, true)?;
        scratch.editor_revision = current_store.editor_revision;
        // 프리셋 스냅샷은 현재 store와 id 세대가 달라 ID 짝짓기가 성립하지 않는다
        validate_document_transition_with_keying(
            &current,
            &candidate,
            &current_store,
            &scratch,
            GrandfatherKeying::LegacyPresetModeIndex,
        )?;

        let changed_fields = current.changed_fields(&candidate);
        let revision = if changed_fields.is_empty() {
            current_store.editor_revision
        } else {
            let revision = next_revision(current_store.editor_revision)?;
            scratch.editor_revision = revision;
            revision
        };
        let selected_key_type = scratch.selected_key_type.clone();
        let key_counters = scratch.key_counters.clone();
        self.commit_locked(guard, scratch, ())
            .map_err(|error| EditorCommitError::io(error.to_string()))?;

        if changed_fields.is_empty() {
            return Ok((None, settings_diff));
        }
        let event = origin.event_name().map(|origin| EditorCommittedV1 {
            schema_version: EDITOR_SCHEMA_VERSION,
            revision,
            mutation_id: operation_id.to_string(),
            gesture_id: None,
            gesture_ids: Vec::new(),
            origin,
            changed_fields: changed_fields.clone(),
            patch: candidate.patch_for_fields(&changed_fields),
        });
        Ok((
            Some(CommittedEditorChange {
                result: EditorCommitResult {
                    revision,
                    changed_fields,
                    op_results: None,
                },
                event,
                replayed: false,
                document: candidate,
                selected_key_type,
                key_counters,
                history_status: None,
                plugin_instances_changes: Vec::new(),
                runtime_publication_generation: 0,
            }),
            settings_diff,
        ))
    }
}

impl HistoryRestorer {
    fn commit_locked(
        &self,
        guard: &mut HistoryDraft,
        store: AppStoreData,
        _value: (),
    ) -> Result<(), String> {
        guard.store = store;
        Ok(())
    }
    fn commit_editor_patch_locked(
        &self,
        guard: &mut HistoryDraft,
        patch: &EditorPatchV1,
        fields: &[EditorField],
        counters: Option<&KeyCounters>,
        options: EditorPatchCommitOptions,
    ) -> Result<CommittedEditorChange, EditorCommitError> {
        let mut current_store = guard.store.clone();
        if let Some(counters) = counters {
            current_store.key_counters = counters.clone();
        }
        let (current, candidate, scratch, changed) =
            prepare_editor_patch_transition(&current_store, patch, fields)?;
        if options.enforce_touched_fields && changed.iter().any(|field| !fields.contains(field)) {
            return Err(EditorCommitError::validation(
                "HISTORY_RESTORE_CHANGED_UNDECLARED_FIELD",
                "history restore changed an editor field outside its entry",
            ));
        }
        let mut prepared = prepare_editor_commit(
            &guard.history,
            0,
            current_store,
            current,
            candidate,
            scratch,
            changed,
            None,
            options,
        )?;
        if let Some(store) = prepared.take_pending_store() {
            guard.store = store;
        }
        Ok(prepared.finalize(&mut guard.history, false, 0))
    }
}

/// 저장 성공 전까지 확정하지 않는 복원 후보와 실패 시 barrier 상태.
pub struct PreparedHistoryOperation {
    pub store: AppStoreData,
    pub history: HistoryService,
    pub failure_history: HistoryService,
    pub plugin_revision: u64,
    pub started: bool,
    pub outcome: Result<HistoryOperationResult, String>,
}
pub fn prepare_history_operation(
    store: &AppStoreData,
    history: &HistoryService,
    plugin_revision: u64,
    direction: HistoryDirection,
    operation_id: &str,
    counters: &KeyCounters,
) -> PreparedHistoryOperation {
    let mut draft = HistoryDraft {
        store: store.clone(),
        history: history.clone(),
        plugin_revision,
    };
    let outcome =
        HistoryRestorer.apply_history_operation(direction, operation_id, counters, &mut draft);
    let started = draft.history.history_epoch() != history.history_epoch();
    let mut failure_history = history.clone();
    if started {
        failure_history.begin_barrier();
        failure_history.finish_barrier();
    }
    PreparedHistoryOperation {
        store: draft.store,
        history: draft.history,
        failure_history,
        plugin_revision: draft.plugin_revision,
        started,
        outcome,
    }
}
