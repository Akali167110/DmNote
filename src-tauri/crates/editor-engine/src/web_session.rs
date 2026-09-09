//! 웹 호스트의 커맨드 세션. 영속 쓰기 성공 전에는 draft를 외부에 공개하지 않는다.
use crate::{
    errors::EditorCommitError,
    models::*,
    state::{editor::*, history::*, plugin::*},
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

mod edits;
mod history;
mod plugins;
mod reads;
mod resources;
mod tabs;

#[derive(Clone)]
struct GestureAck {
    id: String,
    fingerprint: RequestFingerprint,
    result: GestureCommitResult,
}
#[derive(Clone)]
struct SessionState {
    store: AppStoreData,
    history: HistoryService,
    editor_acks: VecDeque<crate::commit::MutationAck>,
    gesture_acks: VecDeque<GestureAck>,
    plugin_revision: u64,
    authority_generation: u64,
    authority_available: bool,
    counters_revision: u64,
    counters_session_id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WebCheckpoint {
    schema_version: u16,
    editor_revision: u64,
    plugin_model_revision: u64,
    authority_generation: u64,
    authority_available: bool,
    counters_revision: u64,
    counters_session_id: String,
    #[serde(default)]
    history_revision: u64,
    #[serde(default)]
    history_epoch: u64,
    #[serde(default)]
    history_status_seq: u64,
}
impl SessionState {
    fn checkpoint(&self) -> WebCheckpoint {
        WebCheckpoint {
            schema_version: 1,
            editor_revision: self.store.editor_revision,
            plugin_model_revision: self.plugin_revision,
            authority_generation: self.authority_generation,
            authority_available: self.authority_available,
            counters_revision: self.counters_revision,
            counters_session_id: self.counters_session_id.clone(),
            history_revision: self.history.status(false).history_revision,
            history_epoch: self.history.history_epoch(),
            history_status_seq: self.history.status(false).status_seq,
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebEvent {
    pub event: String,
    pub payload: Value,
}
#[derive(Default)]
struct Output {
    result: Value,
    events: Vec<WebEvent>,
    asset_writes: std::collections::BTreeMap<String, String>,
    asset_deletes: Vec<String>,
}
struct Pending {
    token: String,
    state: SessionState,
    response: String,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct WebEditorSession {
    state: SessionState,
    pending: Option<Pending>,
    can_restore_checkpoint: bool,
}
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl WebEditorSession {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new(store_json: &str) -> Result<Self, String> {
        let store: AppStoreData = serde_json::from_str(store_json).map_err(|e| e.to_string())?;
        validate_revision(store.editor_revision).map_err(wire_error)?;
        Ok(Self {
            state: SessionState {
                store,
                history: HistoryService::default(),
                editor_acks: VecDeque::new(),
                gesture_acks: VecDeque::new(),
                plugin_revision: 0,
                authority_generation: 0,
                authority_available: false,
                counters_revision: 0,
                counters_session_id: uuid::Uuid::new_v4().to_string(),
            },
            pending: None,
            can_restore_checkpoint: true,
        })
    }
    pub fn restore_checkpoint(&mut self, checkpoint_json: &str) -> Result<(), String> {
        if !self.can_restore_checkpoint || self.pending.is_some() {
            return Err("WEB_CHECKPOINT_ALREADY_INITIALIZED".into());
        }
        let checkpoint: WebCheckpoint = serde_json::from_str(checkpoint_json)
            .map_err(|e| format!("INVALID_WEB_CHECKPOINT:{e}"))?;
        if checkpoint.schema_version != 1
            || checkpoint.editor_revision != self.state.store.editor_revision
            || uuid::Uuid::parse_str(&checkpoint.counters_session_id).is_err()
        {
            return Err("INVALID_WEB_CHECKPOINT".into());
        }
        for revision in [
            checkpoint.plugin_model_revision,
            checkpoint.authority_generation,
            checkpoint.counters_revision,
        ] {
            validate_revision(revision).map_err(wire_error)?;
        }
        for revision in [
            checkpoint.history_revision,
            checkpoint.history_epoch,
            checkpoint.history_status_seq,
        ] {
            next_revision(revision).map_err(wire_error)?;
        }
        self.state.history = HistoryService::restart_empty_after(
            checkpoint.history_revision,
            checkpoint.history_epoch,
            checkpoint.history_status_seq,
        );
        self.state.plugin_revision = checkpoint.plugin_model_revision;
        self.state.authority_generation = checkpoint.authority_generation;
        self.state.authority_available = checkpoint.authority_available;
        self.state.counters_revision = checkpoint.counters_revision;
        self.state.counters_session_id = checkpoint.counters_session_id;
        self.can_restore_checkpoint = false;
        Ok(())
    }
    pub fn snapshot(&self) -> Result<String, String> {
        encode(
            &json!({"store":self.state.store,"checkpoint":self.state.checkpoint(),"history":self.state.history.status(false),"pluginModelRevision":self.state.plugin_revision,"authorityGeneration":self.state.authority_generation}),
        )
    }
    pub fn read(&self, command: &str, args_json: &str) -> Result<String, String> {
        let args = parse_args(args_json)?;
        encode(&self.state.read(command, &args)?)
    }
    pub fn prepare_command(
        &mut self,
        command: &str,
        args_json: &str,
        token: &str,
    ) -> Result<String, String> {
        if self.pending.is_some() {
            return Err("EDITOR_SAVE_PENDING".into());
        }
        if token.is_empty() || token.len() > 256 {
            return Err("INVALID_WEB_TRANSACTION_TOKEN".into());
        }
        if web_command_kind(command) != "write" {
            return Err(format!("UNSUPPORTED_WEB_COMMAND:{command}"));
        }
        let args = parse_args(args_json)?;
        let mut draft = self.state.clone();
        let output = draft.execute(command, &args)?;
        let changed = draft.store != self.state.store;
        let response = encode(
            &json!({"token":token,"checkpoint":draft.checkpoint(),"store":if changed {Some(&draft.store)} else {None},"result":output.result,"events":output.events,"assetWrites":output.asset_writes,"assetDeletes":output.asset_deletes}),
        )?;
        self.can_restore_checkpoint = false;
        self.pending = Some(Pending {
            token: token.into(),
            state: draft,
            response: response.clone(),
        });
        Ok(response)
    }
    pub fn confirm_command(&mut self, token: &str) -> Result<String, String> {
        self.validate_token(token)?;
        let pending = self.pending.take().expect("validated pending token");
        self.state = pending.state;
        Ok(pending.response)
    }
    pub fn discard_command(&mut self, token: &str) -> Result<(), String> {
        self.validate_token(token)?;
        self.pending = None;
        Ok(())
    }
}
impl WebEditorSession {
    fn validate_token(&self, token: &str) -> Result<(), String> {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.token == token)
        {
            Ok(())
        } else {
            Err("EDITOR_PENDING_MUTATION_MISMATCH".into())
        }
    }
}
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn web_command_kind(command: &str) -> String {
    match command {
        "app_bootstrap"
        | "editor_get"
        | "history_status"
        | "settings_get"
        | "keys_get"
        | "positions_get"
        | "stat_positions_get"
        | "graph_positions_get"
        | "knob_positions_get"
        | "layer_groups_get"
        | "keys_get_counters"
        | "custom_tabs_list"
        | "note_tab_get"
        | "note_tab_get_all"
        | "plugin_instances_get"
        | "plugin_group_refs_get"
        | "plugin_storage_get"
        | "plugin_storage_keys"
        | "plugin_storage_has_data"
        | "css_get"
        | "css_get_use"
        | "css_history_get"
        | "css_tab_get"
        | "css_tab_get_all"
        | "css_tab_export"
        | "js_get"
        | "js_get_use"
        | "overlay_get"
        | "counter_animation_list"
        | "preset_save"
        | "preset_save_tab"
        | "sound_load_original"
        | "sprite_positions_get" => "read",
        "web_main_disconnected"
        | "web_input_press"
        | "editor_commit"
        | "commit_gesture"
        | "history_undo"
        | "history_redo"
        | "settings_update"
        | "plugin_authority_reset"
        | "plugin_instances_commit"
        | "plugin_instances_reconcile"
        | "plugin_storage_set"
        | "plugin_storage_remove"
        | "plugin_storage_clear"
        | "plugin_storage_clear_by_prefix"
        | "keys_set_mode"
        | "custom_tabs_select"
        | "custom_tabs_create"
        | "custom_tabs_rename"
        | "custom_tabs_delete"
        | "custom_tabs_restore"
        | "tabs_reorder"
        | "keys_reset_all"
        | "keys_reset_mode"
        | "keys_reset_counters"
        | "keys_reset_counters_mode"
        | "keys_reset_single_counter"
        | "keys_set_counters"
        | "note_tab_set"
        | "note_tab_clear"
        | "css_toggle"
        | "css_set_content"
        | "css_reset"
        | "css_load"
        | "css_tab_set"
        | "css_tab_toggle"
        | "css_tab_clear"
        | "css_tab_load"
        | "css_history_remove"
        | "css_history_activate"
        | "css_tab_activate_history"
        | "js_toggle"
        | "js_reset"
        | "js_set_content"
        | "js_load"
        | "js_reload"
        | "js_remove_plugin"
        | "js_set_plugin_enabled"
        | "counter_animation_create"
        | "counter_animation_update"
        | "counter_animation_delete"
        | "overlay_set_lock"
        | "overlay_set_visible"
        | "overlay_set_anchor"
        | "preset_load"
        | "preset_load_tab"
        | "sound_set_hidden"
        | "sound_set_enabled"
        | "sound_rename"
        | "sound_delete"
        | "sound_list"
        | "font_load"
        | "image_load"
        | "sound_load"
        | "sound_save_processed_wav"
        | "sound_update_processed_wav" => "write",
        _ => "unsupported",
    }
    .into()
}
fn parse_args(value: &str) -> Result<Value, String> {
    let args: Value =
        serde_json::from_str(value).map_err(|e| format!("INVALID_COMMAND_ARGS:{e}"))?;
    if args.is_object() {
        Ok(args)
    } else {
        Err("INVALID_COMMAND_ARGS".into())
    }
}
fn encode<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}
fn value<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}
fn arg<T: DeserializeOwned>(args: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(
        args.get(key)
            .cloned()
            .ok_or_else(|| format!("MISSING_COMMAND_ARG:{key}"))?,
    )
    .map_err(|e| format!("INVALID_COMMAND_ARG:{key}:{e}"))
}
fn optional<T: DeserializeOwned>(args: &Value, key: &str) -> Result<Option<T>, String> {
    args.get(key)
        .filter(|v| !v.is_null())
        .cloned()
        .map(|v| serde_json::from_value(v).map_err(|e| format!("INVALID_COMMAND_ARG:{key}:{e}")))
        .transpose()
}
fn wire_error(error: EditorCommitError) -> String {
    serde_json::to_string(&error).expect("serializable editor error")
}
impl Output {
    fn event(&mut self, event: &str, payload: impl Serialize) -> Result<(), String> {
        self.events.push(WebEvent {
            event: event.into(),
            payload: value(payload)?,
        });
        Ok(())
    }
}
impl SessionState {
    fn history_event(&mut self, out: &mut Output) -> Result<(), String> {
        out.event("history:status", self.history.issue_status(false))
    }
    fn observed_epoch(&self, args: &Value) -> Result<(), String> {
        if optional::<u64>(args, "observedHistoryEpoch")?
            .is_some_and(|v| v != self.history.history_epoch())
        {
            Err(wire_error(EditorCommitError::history_epoch_conflict(
                self.history.history_epoch(),
            )))
        } else {
            Ok(())
        }
    }
    fn authority(&self, generation: u64) -> Result<(), String> {
        if !self.authority_available {
            Err("AUTHORITY_UNAVAILABLE".into())
        } else if generation != self.authority_generation {
            Err("AUTHORITY_GENERATION_CHANGED".into())
        } else {
            Ok(())
        }
    }
    fn execute(&mut self, command: &str, args: &Value) -> Result<Output, String> {
        match command {
            "web_main_disconnected" => {
                self.authority_available = false;
                Ok(Output::default())
            }
            "web_input_press" => self.input_press(args),
            "editor_commit" => self.editor_commit(args),
            "commit_gesture" => self.gesture_commit(args),
            "history_undo" | "history_redo" => self.restore_history(command, args),
            cmd if cmd.starts_with("plugin_") => self.plugin_command(command, args),
            cmd if cmd.starts_with("keys_")
                || cmd.starts_with("custom_tabs_")
                || cmd == "tabs_reorder" =>
            {
                self.tabs_command(command, args)
            }
            _ => self.resource_command(command, args),
        }
    }
    fn publish_editor(
        &mut self,
        out: &mut Output,
        event: Option<EditorCommittedV1>,
        fields: &[EditorField],
        legacy: bool,
    ) -> Result<(), String> {
        if let Some(event) = event {
            out.event("editor:committed", event)?;
        }
        if legacy {
            for field in fields {
                match field {
                    EditorField::Keys => out.event("keys:changed", &self.store.keys)?,
                    EditorField::KeyPositions => {
                        out.event("positions:changed", &self.store.key_positions)?
                    }
                    EditorField::StatPositions => {
                        out.event("statPositions:changed", &self.store.stat_positions)?
                    }
                    EditorField::GraphPositions => {
                        out.event("graphPositions:changed", &self.store.graph_positions)?
                    }
                    EditorField::KnobPositions => {
                        out.event("knobPositions:changed", &self.store.knob_positions)?
                    }
                    EditorField::SpritePositions => {
                        out.event("spritePositions:changed", &self.store.sprite_positions)?
                    }
                    EditorField::LayerGroups => {
                        out.event("layerGroups:changed", &self.store.layer_groups)?
                    }
                }
            }
        }
        Ok(())
    }
    fn overlap_changed(
        &mut self,
        before: &PresetFullHistorySnapshot,
        out: &mut Output,
    ) -> Result<(), String> {
        if !before.matches_store(&self.store) && self.history.invalidate_future() {
            self.history_event(out)?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests;
