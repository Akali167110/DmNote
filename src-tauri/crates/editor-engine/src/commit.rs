use crate::{
    errors::EditorCommitError,
    models::*,
    state::{
        editor::*,
        history::{HistoryRecordPlan, HistoryService},
    },
};
pub struct EditorPatchCommitOptions {
    pub mutation_id: String,
    pub gesture_id: Option<String>,
    pub gesture_ids: Vec<String>,
    pub origin: EditorCommitOrigin,
    pub record_history: bool,
    pub apply_key_side_effects: bool,
    pub enforce_touched_fields: bool,
}

pub fn prepare_editor_patch_transition(
    current_store: &AppStoreData,
    changes: &crate::models::EditorPatchV1,
    touched_fields: &[EditorField],
) -> std::result::Result<
    (
        EditorDocumentV1,
        EditorDocumentV1,
        AppStoreData,
        Vec<EditorField>,
    ),
    EditorCommitError,
> {
    let current = EditorDocumentV1::from_store(current_store);
    let mut candidate = current.clone();
    candidate.apply_patch(changes);

    let mut scratch = current_store.clone();
    candidate.apply_to_store(&mut scratch);
    crate::state::migration::canonicalize_gradient_pairs(&mut scratch);
    crate::state::migration::canonicalize_image_modes(&mut scratch);
    crate::state::migration::normalize_sprite_triggers(&mut scratch);
    candidate = EditorDocumentV1::from_store(&scratch);

    validate_paired_update(
        &current,
        &candidate,
        touched_fields.contains(&EditorField::Keys),
        touched_fields.contains(&EditorField::KeyPositions),
    )?;
    scratch.editor_revision = current_store.editor_revision;
    validate_document_transition(&current, &candidate, current_store, &scratch)?;
    let changed_fields = current.changed_fields(&candidate);

    Ok((current, candidate, scratch, changed_fields))
}

/// 준비 결과는 저장 완료까지 호스트의 단일 변경 큐에서 독점 보관한다.
pub struct PreparedEditorCommit {
    scratch: Option<Box<AppStoreData>>,
    change: CommittedEditorChange,
    history_plan: Option<HistoryRecordPlan>,
}
#[allow(clippy::too_many_arguments)]
pub fn prepare_editor_commit(
    history: &HistoryService,
    runtime_publication_generation: u64,
    current_store: AppStoreData,
    current: EditorDocumentV1,
    candidate: EditorDocumentV1,
    mut scratch: AppStoreData,
    changed_fields: Vec<EditorField>,
    op_results: Option<Vec<EditorOpResultV1>>,
    options: EditorPatchCommitOptions,
) -> Result<PreparedEditorCommit, EditorCommitError> {
    if changed_fields.is_empty() {
        return Ok(PreparedEditorCommit {
            scratch: None,
            history_plan: None,
            change: CommittedEditorChange {
                result: EditorCommitResult {
                    revision: current_store.editor_revision,
                    changed_fields,
                    op_results,
                },
                event: None,
                replayed: false,
                document: current,
                selected_key_type: current_store.selected_key_type,
                key_counters: current_store.key_counters,
                history_status: None,
                plugin_instances_changes: Vec::new(),
                runtime_publication_generation,
            },
        });
    }

    let history_plan = options
        .record_history
        .then(|| {
            history.prepare_entry_with_gesture_ids(
                changed_fields.clone(),
                current.patch_for_fields(&changed_fields),
                changed_fields
                    .contains(&EditorField::Keys)
                    .then(|| current_store.key_counters.clone()),
                options.gesture_ids.clone(),
            )
        })
        .transpose()
        .map_err(|error| EditorCommitError::validation("HISTORY_SERIALIZATION_FAILED", error))?;

    let revision = next_revision(current_store.editor_revision)?;
    if options.apply_key_side_effects && changed_fields.contains(&EditorField::Keys) {
        sync_key_counters(&mut scratch.key_counters, &candidate.keys);
        repair_selected_mode(&mut scratch);
    }
    scratch.editor_revision = revision;
    let selected_key_type = scratch.selected_key_type.clone();
    let key_counters = scratch.key_counters.clone();

    let event = options.origin.event_name().map(|origin| EditorCommittedV1 {
        schema_version: EDITOR_SCHEMA_VERSION,
        revision,
        mutation_id: options.mutation_id,
        gesture_id: options.gesture_id,
        gesture_ids: options.gesture_ids,
        origin,
        changed_fields: changed_fields.clone(),
        patch: candidate.patch_for_fields(&changed_fields),
    });
    Ok(PreparedEditorCommit {
        scratch: Some(Box::new(scratch)),
        history_plan,
        change: CommittedEditorChange {
            result: EditorCommitResult {
                revision,
                changed_fields,
                op_results,
            },
            event,
            replayed: false,
            document: candidate,
            selected_key_type,
            key_counters,
            history_status: None,
            plugin_instances_changes: Vec::new(),
            runtime_publication_generation,
        },
    })
}
impl PreparedEditorCommit {
    pub fn pending_store(&self) -> Option<&AppStoreData> {
        self.scratch.as_deref()
    }
    /// 저장 모듈에 소유권 전달. 큰 문서 전체를 다시 복제하지 않는다.
    pub fn take_pending_store(&mut self) -> Option<AppStoreData> {
        self.scratch.take().map(|store| *store)
    }
    /// 영속 저장 성공 후 호출. 실패 가능한 판단은 prepare에서 완료한다.
    pub fn finalize(
        mut self,
        history: &mut HistoryService,
        admission_closed: bool,
        runtime_publication_generation: u64,
    ) -> CommittedEditorChange {
        self.change.history_status = self.history_plan.map(|plan| {
            history.apply_editor_record_plan(plan, &self.change.document);
            history.issue_status(admission_closed)
        });
        self.change.runtime_publication_generation = runtime_publication_generation;
        self.change
    }
}

use std::collections::VecDeque;
#[derive(Clone)]
pub struct MutationAck {
    pub id: String,
    pub fingerprint: RequestFingerprint,
    pub result: EditorCommitResult,
}

pub fn normalize_editor_request(
    request: &mut EditorCommitRequest,
) -> Result<RequestFingerprint, EditorCommitError> {
    if let Some(keys) = request
        .changes
        .as_mut()
        .and_then(|changes| changes.keys.as_mut())
    {
        normalize_key_mappings(keys);
    }
    validate_request_envelope(request)?;
    request_fingerprint(request)
}

pub fn validate_editor_request_state(
    store: &AppStoreData,
    request: &EditorCommitRequest,
    acks: &VecDeque<MutationAck>,
    fingerprint: &RequestFingerprint,
) -> Result<Option<EditorCommitResult>, EditorCommitError> {
    if let Some(ack) = acks.iter().find(|ack| ack.id == request.mutation_id) {
        if ack.fingerprint != *fingerprint {
            return Err(EditorCommitError::mutation_id_reused());
        }
        return Ok(Some(ack.result.clone()));
    }
    if request.base_revision != store.editor_revision {
        return Err(EditorCommitError::revision_conflict(store.editor_revision));
    }
    if request
        .changes
        .as_ref()
        .is_some_and(|changes| changes.keys.is_some())
        && key_mappings_contain_multi(&store.keys)
        && !request.multi_key
    {
        return Err(EditorCommitError::multi_key_unsupported());
    }
    Ok(None)
}

pub struct EditorRequestTransition {
    pub current: EditorDocumentV1,
    pub candidate: EditorDocumentV1,
    pub scratch: AppStoreData,
    pub changed_fields: Vec<EditorField>,
    pub op_results: Option<Vec<EditorOpResultV1>>,
}

pub fn prepare_editor_request_transition(
    store: &AppStoreData,
    request: &mut EditorCommitRequest,
) -> Result<EditorRequestTransition, EditorCommitError> {
    if let Some(changes) = request.changes.as_mut() {
        changes.merge_omitted_sprite_fields(&store.sprite_positions);
        crate::state::native_element_id::prepare_commit_patch_element_ids(store, changes)?;
        let (current, candidate, scratch, changed_fields) =
            prepare_editor_patch_transition(store, changes, &changes.included_fields())?;
        Ok(EditorRequestTransition {
            current,
            candidate,
            scratch,
            changed_fields,
            op_results: None,
        })
    } else if let Some(ops) = request.ops.as_ref() {
        let transition = crate::state::editor_ops::prepare_editor_ops_transition(store, ops)?;
        Ok(EditorRequestTransition {
            current: transition.current,
            candidate: transition.candidate,
            scratch: transition.scratch,
            changed_fields: transition.changed_fields,
            op_results: Some(transition.op_results),
        })
    } else {
        Err(EditorCommitError::validation(
            "EDITOR_MUTATION_REQUIRED",
            "editor commit must contain exactly one mutation payload",
        ))
    }
}
