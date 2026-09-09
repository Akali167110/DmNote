use super::*;
impl AppStore {
    pub(crate) fn apply_history_operation(
        &self,
        direction: HistoryDirection,
        operation_id: &str,
        current_key_counters: &KeyCounters,
        cancel_previews: impl FnOnce(),
    ) -> Result<HistoryOperationResult, String> {
        if operation_id.len() > 64 || uuid::Uuid::parse_str(operation_id).is_err() {
            return Err(INVALID_HISTORY_OPERATION_ID.to_string());
        }
        let mut guard = self.lock_for_update().map_err(|e| e.to_string())?;
        let prepared = dmnote_editor_engine::history_transition::prepare_history_operation(
            &guard.data,
            &guard.history,
            guard.plugin_model_revision,
            direction,
            operation_id,
            current_key_counters,
        );
        if prepared.started {
            cancel_previews();
        }
        let mut outcome = match prepared.outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                guard.history = prepared.history;
                return Err(error);
            }
        };
        if !outcome.replayed {
            if let Err(error) = self.commit_locked(&mut guard, prepared.store, ()) {
                guard.history = prepared.failure_history;
                return Err(error.to_string());
            }
            guard.history = prepared.history;
            guard.plugin_model_revision = prepared.plugin_revision;
        }
        outcome.runtime_publication_generation = guard.revision;
        if let Some(change) = outcome.change.as_mut() {
            change.runtime_publication_generation = guard.revision;
        }
        Ok(outcome)
    }
}
