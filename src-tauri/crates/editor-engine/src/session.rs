//! 브라우저 호스트용 JSON 호출 경계. 저장 성공은 호스트가 명시적으로 확인한다.
use std::collections::VecDeque;

use serde_json::json;

use crate::{
    commit::{prepare_editor_commit, EditorPatchCommitOptions, PreparedEditorCommit},
    errors::EditorCommitError,
    models::*,
    state::{editor::*, history::HistoryService},
};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

struct PendingCommit {
    mutation_id: String,
    fingerprint: RequestFingerprint,
    prepared: PreparedEditorCommit,
}

use crate::commit::{
    normalize_editor_request, prepare_editor_request_transition, validate_editor_request_state,
    MutationAck,
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct EditorSession {
    state: AppStoreData,
    history: HistoryService,
    pending: Option<PendingCommit>,
    acks: VecDeque<MutationAck>,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl EditorSession {
    /// 마이그레이션과 자산 복원을 완료한 저장 상태로 세션 구성.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new(store_json: &str) -> Result<EditorSession, String> {
        let state: AppStoreData =
            serde_json::from_str(store_json).map_err(|error| error.to_string())?;
        validate_revision(state.editor_revision).map_err(wire_error)?;
        Ok(Self {
            state,
            history: HistoryService::default(),
            pending: None,
            acks: VecDeque::new(),
        })
    }

    pub fn snapshot(&self) -> Result<String, String> {
        serde_json::to_string(
            &json!({ "store": self.state, "history": self.history.status(false) }),
        )
        .map_err(|error| error.to_string())
    }

    /// 반환된 store를 호스트가 저장한 뒤 confirm 호출. 실패 시 discard 호출.
    pub fn prepare(&mut self, request_json: &str) -> Result<String, String> {
        if self.pending.is_some() {
            return Err("EDITOR_SAVE_PENDING".to_string());
        }
        let value = serde_json::from_str(request_json).map_err(|error| {
            wire_error(EditorCommitError::validation(
                "INVALID_REQUEST_PAYLOAD",
                error.to_string(),
            ))
        })?;
        let mut request = decode_editor_commit_request(value).map_err(wire_error)?;
        request_payload_size(&request).map_err(wire_error)?;
        let fingerprint = normalize_editor_request(&mut request).map_err(wire_error)?;
        if let Some(result) =
            validate_editor_request_state(&self.state, &request, &self.acks, &fingerprint)
                .map_err(wire_error)?
        {
            return serde_json::to_string(&json!({"mutationId": request.mutation_id, "replayed": true, "result": result, "store": null})).map_err(|error| error.to_string());
        }
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
            prepare_editor_request_transition(&self.state, &mut request).map_err(wire_error)?;
        let prepared = prepare_editor_commit(
            &self.history,
            0,
            self.state.clone(),
            transition.current,
            transition.candidate,
            transition.scratch,
            transition.changed_fields,
            transition.op_results,
            options,
        )
        .map_err(wire_error)?;
        let response = serde_json::to_string(&json!({"mutationId": request.mutation_id, "replayed": false, "store": prepared.pending_store()})).map_err(|error| error.to_string())?;
        self.pending = Some(PendingCommit {
            mutation_id: request.mutation_id,
            fingerprint,
            prepared,
        });
        Ok(response)
    }

    pub fn confirm(&mut self, mutation_id: &str) -> Result<String, String> {
        self.validate_pending(mutation_id)?;
        let mut pending = self.pending.take().expect("pending mutation validated");
        if let Some(store) = pending.prepared.take_pending_store() {
            self.state = store;
        }
        let change = pending.prepared.finalize(&mut self.history, false, 0);
        if self.acks.len() == MUTATION_ACK_CAPACITY {
            self.acks.pop_front();
        }
        self.acks.push_back(MutationAck {
            id: pending.mutation_id,
            fingerprint: pending.fingerprint,
            result: change.result.clone(),
        });
        serde_json::to_string(&json!({"result": change.result, "event": change.event, "history": change.history_status})).map_err(|error| error.to_string())
    }

    pub fn discard(&mut self, mutation_id: &str) -> Result<(), String> {
        self.validate_pending(mutation_id)?;
        self.pending = None;
        Ok(())
    }
}

impl EditorSession {
    fn validate_pending(&self, mutation_id: &str) -> Result<(), String> {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.mutation_id == mutation_id)
        {
            Ok(())
        } else {
            Err("EDITOR_PENDING_MUTATION_MISMATCH".to_string())
        }
    }
}

fn wire_error(error: EditorCommitError) -> String {
    serde_json::to_string(&error).expect("editor error payload is serializable")
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn decode_preset_json(content: &str) -> Result<String, String> {
    let preset = crate::preset::validation::decode_preset(content)?;
    serde_json::to_string(&preset).map_err(|error| error.to_string())
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn migrate_store_json(content: &str, timestamp: i64) -> Result<String, String> {
    let loaded = crate::state::migration::load_store_bytes(
        content.as_bytes(),
        "web-store",
        timestamp,
        str::to_string,
    );
    serde_json::to_string(&json!({"store": loaded.data, "needsPersist": loaded.needs_persist, "repaired": loaded.repaired})).map_err(|error| error.to_string())
}
