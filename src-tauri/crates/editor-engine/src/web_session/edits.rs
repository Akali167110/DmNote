use super::*;
use crate::commit::*;
use crate::state::{
    editor_ops::prepare_editor_ops_transition_with_plugin_refs, gesture::*, native_element_id,
};
use std::collections::HashSet;
impl SessionState {
    pub(super) fn editor_commit(&mut self, args: &Value) -> Result<Output, String> {
        let mut request =
            decode_editor_commit_request(arg(args, "request")?).map_err(wire_error)?;
        request_payload_size(&request).map_err(wire_error)?;
        let fingerprint = normalize_editor_request(&mut request).map_err(wire_error)?;
        if let Some(result) =
            validate_editor_request_state(&self.store, &request, &self.editor_acks, &fingerprint)
                .map_err(wire_error)?
        {
            return Ok(Output {
                result: value(result)?,
                ..Default::default()
            });
        }
        let requested_fields = request
            .changes
            .as_ref()
            .map_or_else(Vec::new, |changes| changes.included_fields());
        let previous_mode = self.store.selected_key_type.clone();
        let previous_counters = self.store.key_counters.clone();
        let is_ops = request.ops.is_some();
        let options = EditorPatchCommitOptions {
            mutation_id: request.mutation_id.clone(),
            gesture_id: request.history_gesture_id(),
            gesture_ids: request.echoed_gesture_ids(),
            origin: EditorCommitOrigin::StrictEditorCommit,
            record_history: true,
            apply_key_side_effects: true,
            enforce_touched_fields: false,
        };
        let transition =
            prepare_editor_request_transition(&self.store, &mut request).map_err(wire_error)?;
        let mut prepared = prepare_editor_commit(
            &self.history,
            0,
            self.store.clone(),
            transition.current,
            transition.candidate,
            transition.scratch,
            transition.changed_fields,
            transition.op_results,
            options,
        )
        .map_err(wire_error)?;
        if let Some(store) = prepared.take_pending_store() {
            self.store = store;
        }
        let change = prepared.finalize(&mut self.history, false, 0);
        if self.editor_acks.len() == MUTATION_ACK_CAPACITY {
            self.editor_acks.pop_front();
        }
        self.editor_acks.push_back(MutationAck {
            id: request.mutation_id,
            fingerprint,
            result: change.result.clone(),
        });
        let mut out = Output {
            result: value(&change.result)?,
            ..Default::default()
        };
        let legacy_fields = if is_ops {
            &change.result.changed_fields
        } else {
            &requested_fields
        };
        self.publish_editor(&mut out, change.event, legacy_fields, true)?;
        if requested_fields.contains(&EditorField::Keys)
            && previous_mode != self.store.selected_key_type
        {
            out.event(
                "keys:mode-changed",
                json!({"mode":self.store.selected_key_type}),
            )?;
        }
        if previous_counters != self.store.key_counters {
            self.counter_event(&mut out)?;
        }
        if let Some(status) = change.history_status {
            out.event("history:status", status)?;
        }
        Ok(out)
    }
    pub(super) fn gesture_commit(&mut self, args: &Value) -> Result<Output, String> {
        let mut request =
            decode_gesture_commit_request(arg(args, "request")?).map_err(wire_error)?;
        validate_gesture_commit_request(&request).map_err(wire_error)?;
        self.authority(request.authority_generation)?;
        self.gesture_inner(&mut request).map_err(wire_error)
    }
    fn gesture_inner(
        &mut self,
        request: &mut GestureCommitRequest,
    ) -> Result<Output, EditorCommitError> {
        let fingerprint = gesture_request_fingerprint(request)?;
        if let Some(ack) = self
            .gesture_acks
            .iter()
            .find(|ack| ack.id == request.mutation_id)
        {
            if ack.fingerprint != fingerprint {
                return Err(EditorCommitError::mutation_id_reused());
            }
            return Ok(Output {
                result: serde_json::to_value(&ack.result).unwrap(),
                ..Default::default()
            });
        }
        if request
            .observed_history_epoch
            .is_some_and(|epoch| epoch != self.history.history_epoch())
        {
            return Err(EditorCommitError::history_epoch_conflict(
                self.history.history_epoch(),
            ));
        }
        if request.editor_base_revision != self.store.editor_revision {
            return Err(EditorCommitError::revision_conflict(
                self.store.editor_revision,
            ));
        }
        if request.plugin_base_revision != self.plugin_revision {
            return Err(EditorCommitError::plugin_revision_conflict(
                self.plugin_revision,
            ));
        }
        if let Some(changes) = request.editor_changes.as_mut() {
            changes.merge_omitted_sprite_fields(&self.store.sprite_positions);
            native_element_id::prepare_commit_patch_element_ids(&self.store, changes)?;
        }

        let current_store = self.store.clone();
        let (current_editor, candidate_editor, mut scratch, changed_fields, editor_op_results) =
            if let Some(changes) = request.editor_changes.as_ref() {
                let touched_fields = changes.included_fields();
                let (current, candidate, scratch, changed_fields) =
                    prepare_editor_patch_transition(&current_store, changes, &touched_fields)?;
                (current, candidate, scratch, changed_fields, None)
            } else if let Some(ops) = request.editor_ops.as_ref() {
                // editor op 적용이 pluginChanges보다 먼저다 - 그룹 생존 판정은
                // 요청 동봉 plugin_changes(커밋 후 상태)를 우선, 미동봉 플러그인만 store
                let request_plugin_ids = request
                    .plugin_changes
                    .iter()
                    .map(|change| change.plugin_id.as_str())
                    .collect::<HashSet<_>>();
                let mut plugin_group_refs =
                    plugin_group_refs_from_store(&current_store, &request_plugin_ids);
                for change in &request.plugin_changes {
                    add_plugin_group_refs(&mut plugin_group_refs, &change.instances);
                }
                let transition = prepare_editor_ops_transition_with_plugin_refs(
                    &current_store,
                    ops,
                    &plugin_group_refs,
                )?;
                (
                    transition.current,
                    transition.candidate,
                    transition.scratch,
                    transition.changed_fields,
                    Some(transition.op_results),
                )
            } else {
                let current = EditorDocumentV1::from_store(&current_store);
                (
                    current.clone(),
                    current,
                    current_store.clone(),
                    Vec::new(),
                    None,
                )
            };

        let mut history_snapshots = Vec::with_capacity(request.plugin_changes.len() + 1);
        if !changed_fields.is_empty() {
            history_snapshots.push(HistorySnapshot::Editor {
                changed_fields: changed_fields.clone(),
                before: Box::new(current_editor.patch_for_fields(&changed_fields)),
                key_counters: changed_fields
                    .contains(&EditorField::Keys)
                    .then(|| current_store.key_counters.clone()),
            });
        }

        let mut changed_plugin_ids = Vec::new();
        for plugin_change in &request.plugin_changes {
            let current_snapshot =
                plugin_elements_snapshot(&current_store, &plugin_change.plugin_id).map_err(
                    |error| EditorCommitError::validation("INVALID_GESTURE_PLUGIN", error),
                )?;
            validate_plugin_instances_transition(
                current_snapshot.instances.as_deref().unwrap_or_default(),
                &plugin_change.instances,
            )
            .map_err(|error| {
                EditorCommitError::validation(
                    error.clone(),
                    format!(
                        "invalid plugin gesture transition '{}': {error}",
                        plugin_change.plugin_id
                    ),
                )
            })?;
            let canonical = PluginElementsHistorySnapshot {
                plugin_id: plugin_change.plugin_id.clone(),
                instances: (!plugin_change.instances.is_empty())
                    .then_some(plugin_change.instances.clone()),
            };
            if current_snapshot == canonical {
                continue;
            }
            apply_plugin_elements_snapshot(&mut scratch, &canonical)
                .map_err(|error| EditorCommitError::validation("INVALID_GESTURE_PLUGIN", error))?;
            history_snapshots.push(HistorySnapshot::PluginElements(current_snapshot));
            changed_plugin_ids.push(plugin_change.plugin_id.clone());
        }

        let editor_revision = if changed_fields.is_empty() {
            current_store.editor_revision
        } else {
            let revision = next_revision(current_store.editor_revision)?;
            if changed_fields.contains(&EditorField::Keys) {
                sync_key_counters(&mut scratch.key_counters, &candidate_editor.keys);
                repair_selected_mode(&mut scratch);
            }
            scratch.editor_revision = revision;
            revision
        };
        let plugin_model_revision = if changed_plugin_ids.is_empty() {
            self.plugin_revision
        } else {
            next_plugin_model_revision(self.plugin_revision).map_err(|error| {
                EditorCommitError::validation("PLUGIN_MODEL_REVISION_OUT_OF_RANGE", error)
            })?
        };
        let result = GestureCommitResult {
            editor_revision,
            changed_fields: changed_fields.clone(),
            editor_op_results: editor_op_results.clone(),
            plugin_model_revision,
            changed_plugin_ids: changed_plugin_ids.clone(),
            authority_generation: request.authority_generation,
        };

        let mut out = Output {
            result: serde_json::to_value(&result).unwrap(),
            ..Default::default()
        };
        if !history_snapshots.is_empty() {
            let plan = self
                .history
                .prepare_gesture_entry(history_snapshots, request.gesture_id.clone())
                .map_err(|e| EditorCommitError::validation("HISTORY_SERIALIZATION_FAILED", e))?;
            if matches!(plan, HistoryRecordPlan::Truncate) {
                return Err(EditorCommitError::validation(
                    HISTORY_ENTRY_TOO_LARGE,
                    "gesture history entry exceeds the size limit",
                ));
            }
            let previous_counters = self.store.key_counters.clone();
            let previous_mode = self.store.selected_key_type.clone();
            self.store = scratch;
            self.plugin_revision = plugin_model_revision;
            self.history.apply_record_plan(plan);
            let event = (!changed_fields.is_empty()).then(|| EditorCommittedV1 {
                schema_version: EDITOR_SCHEMA_VERSION,
                revision: editor_revision,
                mutation_id: request.mutation_id.clone(),
                gesture_id: Some(request.gesture_id.clone()),
                gesture_ids: vec![request.gesture_id.clone()],
                origin: EditorCommitOrigin::GestureCommit.event_name().unwrap(),
                changed_fields: changed_fields.clone(),
                patch: candidate_editor.patch_for_fields(&changed_fields),
            });
            let requested_fields = request.editor_changes.as_ref().map_or_else(
                || changed_fields.clone(),
                |changes| changes.included_fields(),
            );
            self.publish_editor(
                &mut out,
                event,
                &requested_fields,
                !changed_fields.is_empty(),
            )
            .map_err(|e| EditorCommitError::validation("EVENT_SERIALIZATION_FAILED", e))?;
            if previous_mode != self.store.selected_key_type
                && requested_fields.contains(&EditorField::Keys)
            {
                out.event(
                    "keys:mode-changed",
                    json!({"mode":self.store.selected_key_type}),
                )
                .unwrap();
            }
            if previous_counters != self.store.key_counters {
                self.counter_event(&mut out).unwrap();
            }
            for plugin_id in changed_plugin_ids {
                out.event(
                    "pluginInstances:changed",
                    PluginInstancesChangedPayload {
                        plugin_id,
                        revision: plugin_model_revision,
                        origin_mutation_id: Some(request.mutation_id.clone()),
                    },
                )
                .unwrap();
            }
            self.history_event(&mut out).unwrap();
        }
        if self.gesture_acks.len() == MUTATION_ACK_CAPACITY {
            self.gesture_acks.pop_front();
        }
        self.gesture_acks.push_back(GestureAck {
            id: request.mutation_id.clone(),
            fingerprint,
            result,
        });
        Ok(out)
    }
}

impl SessionState {
    pub(super) fn input_press(&mut self, args: &Value) -> Result<Output, String> {
        let mode: String = arg(args, "mode")?;
        let keys: Vec<String> = arg(args, "keys")?;
        let mut out = Output::default();
        if self.store.key_counter_enabled {
            for key in keys {
                let count = self
                    .store
                    .key_counters
                    .entry(mode.clone())
                    .or_default()
                    .entry(key.clone())
                    .or_default();
                *count = count.saturating_add(1);
                self.counters_revision = self.counters_revision.saturating_add(1);
                out.event("keys:counter",json!({"mode":mode,"key":key,"count":count,"sessionId":self.counters_session_id,"revision":self.counters_revision}))?;
            }
        }
        Ok(out)
    }
}
