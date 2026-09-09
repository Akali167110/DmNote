use super::*;

impl AppStore {
    #[cfg(test)]
    pub fn commit_editor_document(
        &self,
        request: EditorCommitRequest,
    ) -> std::result::Result<CommittedEditorChange, EditorCommitError> {
        let admission = self.admit_editor_mutation()?;
        if request.may_change_keys() {
            let counters = self.snapshot().key_counters;
            self.commit_editor_document_with_runtime_counters_admitted(
                request, &admission, &counters,
            )
        } else {
            self.commit_editor_document_admitted(request, &admission)
        }
    }

    pub(crate) fn admit_editor_mutation(
        &self,
    ) -> std::result::Result<HistoryAdmissionLease, EditorCommitError> {
        self.history_gate
            .admit_mutation()
            .map_err(|error| match error.as_str() {
                HISTORY_IN_PROGRESS => EditorCommitError::history_in_progress(),
                _ => EditorCommitError::io(error),
            })
    }

    pub(crate) fn commit_editor_document_admitted(
        &self,
        request: EditorCommitRequest,
        admission: &HistoryAdmissionLease,
    ) -> std::result::Result<CommittedEditorChange, EditorCommitError> {
        if request.may_change_keys() {
            return Err(key_counter_baseline_required());
        }
        self.commit_editor_document_admitted_with_runtime_counters(request, admission, None)
    }

    pub(crate) fn commit_editor_document_with_runtime_counters_admitted(
        &self,
        request: EditorCommitRequest,
        admission: &HistoryAdmissionLease,
        runtime_counters: &KeyCounters,
    ) -> std::result::Result<CommittedEditorChange, EditorCommitError> {
        self.commit_editor_document_admitted_with_runtime_counters(
            request,
            admission,
            Some(runtime_counters),
        )
    }

    fn commit_editor_document_admitted_with_runtime_counters(
        &self,
        request: EditorCommitRequest,
        admission: &HistoryAdmissionLease,
        runtime_counters: Option<&KeyCounters>,
    ) -> std::result::Result<CommittedEditorChange, EditorCommitError> {
        let started = Instant::now();
        let base_revision = request.base_revision;
        let mutation_id = uuid::Uuid::parse_str(&request.mutation_id)
            .map(|id| id.hyphenated().to_string())
            .unwrap_or_else(|_| "<invalid>".to_string());
        let mutation_kind = if request.ops.is_some() {
            "ops"
        } else {
            "patch"
        };
        let ops_version = request
            .ops_version
            .map_or_else(|| "none".to_string(), |version| version.to_string());
        let op_count = request.ops.as_ref().map_or(0, Vec::len);
        let payload_size = request_payload_size(&request);
        let payload_bytes = payload_size.as_ref().copied().unwrap_or(0);
        let result = match payload_size {
            Ok(_) => self.commit_editor_document_inner(request, admission, runtime_counters),
            Err(error) => Err(error),
        };
        let current_revision = result
            .as_ref()
            .err()
            .and_then(|error| error.details.as_ref())
            .and_then(|details| details.current_revision)
            .unwrap_or_else(|| self.state.read().data.editor_revision);
        let (outcome, changed_fields) = match &result {
            Ok(change) if change.event.is_some() => {
                ("committed", change.result.changed_fields.as_slice())
            }
            Ok(change) if change.result.changed_fields.is_empty() => {
                ("no_op", change.result.changed_fields.as_slice())
            }
            Ok(change) => ("replay", change.result.changed_fields.as_slice()),
            Err(error) => (editor_error_outcome(error.error_code), &[][..]),
        };
        let (applied_count, no_change_count, target_missing_count) = result
            .as_ref()
            .ok()
            .and_then(|change| change.result.op_results.as_ref())
            .map(|results| {
                results
                    .iter()
                    .fold((0, 0, 0), |counts, result| match result.status {
                        EditorOpResultStatusV1::Applied => (counts.0 + 1, counts.1, counts.2),
                        EditorOpResultStatusV1::NoChange => (counts.0, counts.1 + 1, counts.2),
                        EditorOpResultStatusV1::TargetMissing => (counts.0, counts.1, counts.2 + 1),
                    })
            })
            .unwrap_or_default();
        let ack_replay = result.as_ref().is_ok_and(|change| change.replayed);
        let validation_code = result
            .as_ref()
            .err()
            .and_then(|error| error.details.as_ref())
            .and_then(|details| details.validation_code.as_deref())
            .unwrap_or("none");
        // 문서·patch 원문 없이 경계 메타데이터만 기록
        log::info!(
            target: "editor_commit",
            "command=editor_commit mutationId={mutation_id} mutationKind={mutation_kind} opsVersion={ops_version} opCount={op_count} baseRevision={base_revision} currentRevision={current_revision} outcome={outcome} validationCode={validation_code} ackReplay={ack_replay} opApplied={applied_count} opNoChange={no_change_count} opTargetMissing={target_missing_count} changedFields={changed_fields:?} durationMs={} payloadBytes={payload_bytes}",
            started.elapsed().as_millis()
        );
        result
    }

    fn commit_editor_document_inner(
        &self,
        mut request: EditorCommitRequest,
        admission: &HistoryAdmissionLease,
        runtime_counters: Option<&KeyCounters>,
    ) -> std::result::Result<CommittedEditorChange, EditorCommitError> {
        let fingerprint = dmnote_editor_engine::commit::normalize_editor_request(&mut request)?;
        let mut guard = self
            .lock_for_update()
            .map_err(|error| EditorCommitError::io(error.to_string()))?;
        admission
            .revalidate_for(&self.history_gate)
            .map_err(|_| EditorCommitError::history_in_progress())?;

        if let Some(result) = dmnote_editor_engine::commit::validate_editor_request_state(
            &guard.data,
            &request,
            &guard.mutation_acks,
            &fingerprint,
        )? {
            return Ok(CommittedEditorChange {
                result,
                event: None,
                replayed: true,
                document: EditorDocumentV1::from_store(&guard.data),
                selected_key_type: guard.data.selected_key_type.clone(),
                key_counters: guard.data.key_counters.clone(),
                history_status: None,
                plugin_instances_changes: Vec::new(),
                runtime_publication_generation: guard.revision,
            });
        }
        let gesture_id = request.history_gesture_id();
        let gesture_ids = request.echoed_gesture_ids();
        let options = EditorPatchCommitOptions {
            mutation_id: request.mutation_id.clone(),
            gesture_id,
            gesture_ids,
            origin: EditorCommitOrigin::StrictEditorCommit,
            record_history: true,
            apply_key_side_effects: true,
            enforce_touched_fields: false,
        };
        let mut current_store = guard.data.clone();
        if let Some(counters) = runtime_counters {
            current_store.key_counters = counters.clone();
        }
        let transition = dmnote_editor_engine::commit::prepare_editor_request_transition(
            &current_store,
            &mut request,
        )?;
        let change = self.commit_editor_transition_locked(
            &mut guard,
            current_store,
            transition.current,
            transition.candidate,
            transition.scratch,
            transition.changed_fields,
            transition.op_results,
            options,
        )?;
        insert_mutation_ack(
            &mut guard.mutation_acks,
            request.mutation_id,
            fingerprint,
            change.result.clone(),
        );
        Ok(change)
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_editor_transition_locked(
        &self,
        guard: &mut VersionedStoreState,
        current_store: AppStoreData,
        current: EditorDocumentV1,
        candidate: EditorDocumentV1,
        scratch: AppStoreData,
        changed_fields: Vec<EditorField>,
        op_results: Option<Vec<EditorOpResultV1>>,
        options: EditorPatchCommitOptions,
    ) -> std::result::Result<CommittedEditorChange, EditorCommitError> {
        let mut prepared = dmnote_editor_engine::commit::prepare_editor_commit(
            &guard.history,
            guard.revision,
            current_store,
            current,
            candidate,
            scratch,
            changed_fields,
            op_results,
            options,
        )?;
        if let Some(scratch) = prepared.take_pending_store() {
            self.commit_locked(guard, scratch, ())
                .map_err(|error| EditorCommitError::io(error.to_string()))?;
        }
        let revision = guard.revision;
        Ok(prepared.finalize(&mut guard.history, self.history_gate.is_closed(), revision))
    }
}
