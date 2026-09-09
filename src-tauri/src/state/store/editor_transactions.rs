use super::*;
use crate::errors::EditorCommitErrorCode;

pub(super) fn ensure_generic_editor_unchanged(
    before: &AppStoreData,
    after: &AppStoreData,
) -> Result<()> {
    if before.editor_revision != after.editor_revision
        || EditorDocumentV1::from_store(before) != EditorDocumentV1::from_store(after)
    {
        return Err(anyhow!(
            "editor fields must be changed through an editor transaction"
        ));
    }
    Ok(())
}

pub(super) fn editor_error_outcome(code: EditorCommitErrorCode) -> &'static str {
    match code {
        EditorCommitErrorCode::RevisionConflict => "revision_conflict",
        EditorCommitErrorCode::PluginRevisionConflict => "plugin_revision_conflict",
        EditorCommitErrorCode::ValidationFailed => "validation_failed",
        EditorCommitErrorCode::TooManyGestureIds => "too_many_gesture_ids",
        EditorCommitErrorCode::InvalidGestureId => "invalid_gesture_id",
        EditorCommitErrorCode::PairedUpdateRequired => "paired_update_required",
        EditorCommitErrorCode::MultiKeyUnsupported => "multi_key_unsupported",
        EditorCommitErrorCode::MutationIdReused => "mutation_id_reused",
        EditorCommitErrorCode::HistoryInProgress => "history_in_progress",
        EditorCommitErrorCode::HistoryEpochConflict => "history_epoch_conflict",
        EditorCommitErrorCode::IoError => "io_error",
    }
}

pub(super) use dmnote_editor_engine::commit::prepare_editor_patch_transition;

pub(super) fn validate_observed_history_epoch(
    history: &HistoryService,
    observed_history_epoch: Option<u64>,
) -> std::result::Result<(), EditorCommitError> {
    if observed_history_epoch.is_some_and(|observed| observed != history.history_epoch()) {
        return Err(EditorCommitError::history_epoch_conflict(
            history.history_epoch(),
        ));
    }
    Ok(())
}

pub(super) fn insert_mutation_ack(
    acks: &mut VecDeque<MutationAck>,
    id: String,
    fingerprint: RequestFingerprint,
    result: EditorCommitResult,
) {
    if acks.len() == MUTATION_ACK_CAPACITY {
        acks.pop_front();
    }
    acks.push_back(MutationAck {
        id,
        fingerprint,
        result,
    });
}

pub(super) fn insert_gesture_mutation_ack(
    acks: &mut VecDeque<GestureMutationAck>,
    id: String,
    fingerprint: RequestFingerprint,
    result: GestureCommitResult,
) {
    if acks.len() == MUTATION_ACK_CAPACITY {
        acks.pop_front();
    }
    acks.push_back(GestureMutationAck {
        id,
        fingerprint,
        result,
    });
}
