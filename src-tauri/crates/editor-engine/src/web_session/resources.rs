use super::*;
use crate::commit::{prepare_editor_commit, EditorPatchCommitOptions};
use crate::web_preset::WebAssetMap;
use crate::web_resources::{prepare_resources, WebFile};
mod counter;
mod css;
mod js;

impl SessionState {
    pub(super) fn resource_command(
        &mut self,
        command: &str,
        args: &Value,
    ) -> Result<Output, String> {
        self.observed_epoch(args)?;
        let before = PresetFullHistorySnapshot::from_store(&self.store);
        let mut out = match command {
            "preset_load" | "preset_load_tab" => return self.preset_command(command, args),
            "image_load"
            | "font_load"
            | "sound_load"
            | "sound_list"
            | "sound_save_processed_wav"
            | "sound_update_processed_wav"
            | "sound_set_hidden"
            | "sound_set_enabled"
            | "sound_rename"
            | "sound_delete" => {
                let old = self.store.clone();
                let result = prepare_resources(
                    &self.store,
                    command,
                    args,
                    &optional::<Vec<WebFile>>(args, "files")?.unwrap_or_default(),
                    &optional::<WebAssetMap>(args, "assets")?.unwrap_or_default(),
                )?;
                self.store = result.store;
                let mut out = Output {
                    result: result.result,
                    asset_writes: result.asset_writes,
                    asset_deletes: result.asset_deletes,
                    ..Default::default()
                };
                if command == "sound_delete" {
                    self.commit_resource_editor(old, command, false, true, &mut out)?;
                    if self.history.invalidate_all() {
                        self.history_event(&mut out)?;
                    }
                }
                out
            }
            cmd if cmd.starts_with("css_") => self.css_command(command, args)?,
            cmd if cmd.starts_with("js_") => self.js_command(command, args)?,
            cmd if cmd.starts_with("counter_animation_") => {
                return self.counter_command(command, args)
            }
            "settings_update" => {
                let patch: SettingsPatchInput = arg(args, "patch")?;
                let mut diff = crate::settings::apply_patch_to_store(&mut self.store, &patch);
                let result = value(self.store.settings_state())?;
                let mut out = Output {
                    result,
                    ..Default::default()
                };
                if let Some(locked) = diff.changed.overlay_locked {
                    out.event("overlay:lock", json!({"locked":locked}))?;
                }
                diff.full = None;
                out.event("settings:changed", diff)?;
                out
            }
            "note_tab_set" | "note_tab_clear" => {
                let id: String = arg(args, "tabId")?;
                let settings: Option<TabNoteSettings> = if command == "note_tab_clear" {
                    None
                } else {
                    optional(args, "settings")?
                };
                if let Some(settings) = &settings {
                    self.store
                        .tab_note_overrides
                        .insert(id.clone(), settings.clone());
                } else {
                    self.store.tab_note_overrides.remove(&id);
                }
                let mut result = json!({"success":true,"tabId":id});
                if command == "note_tab_set" {
                    if let Some(settings) = &settings {
                        result["settings"] = value(settings)?;
                    }
                }
                let mut out = Output {
                    result,
                    ..Default::default()
                };
                out.event("tabNote:changed", json!({"tabId":id,"settings":settings}))?;
                out
            }
            "overlay_set_lock" => {
                let locked = arg(args, "locked")?;
                self.store.overlay_locked = locked;
                let mut out = Output {
                    result: Value::Null,
                    ..Default::default()
                };
                out.event("overlay:lock", json!({"locked":locked}))?;
                out
            }
            "overlay_set_visible" => {
                let visible = arg(args, "visible")?;
                self.store.overlay_visible = visible;
                let mut out = Output {
                    result: Value::Null,
                    ..Default::default()
                };
                out.event("overlay:visibility", json!({"visible":visible}))?;
                out
            }
            "overlay_set_anchor" => {
                let requested: String = arg(args, "anchor")?;
                let anchor = overlay_resize_anchor_from_str(&requested)
                    .unwrap_or_else(|| self.store.overlay_resize_anchor.clone());
                self.store.overlay_resize_anchor = anchor.clone();
                let mut out = Output {
                    result: json!(anchor.as_str()),
                    ..Default::default()
                };
                out.event("overlay:anchor", json!({"anchor":anchor.as_str()}))?;
                out
            }
            _ => return Err(format!("UNSUPPORTED_WEB_COMMAND:{command}")),
        };
        let prior_event_count = out.events.len();
        self.overlap_changed(&before, &mut out)?;
        if out.events.len() > prior_event_count {
            let history = out.events.pop().expect("history status appended");
            out.events.insert(0, history);
        }
        Ok(out)
    }
    fn commit_resource_editor(
        &mut self,
        before: AppStoreData,
        command: &str,
        record_history: bool,
        legacy: bool,
        out: &mut Output,
    ) -> Result<(), String> {
        crate::state::migration::canonicalize_gradient_pairs(&mut self.store);
        crate::state::migration::canonicalize_image_modes(&mut self.store);
        crate::state::migration::normalize_sprite_triggers(&mut self.store);
        let current = EditorDocumentV1::from_store(&before);
        let candidate = EditorDocumentV1::from_store(&self.store);
        let fields = current.changed_fields(&candidate);
        validate_document_transition_with_keying(
            &current,
            &candidate,
            &before,
            &self.store,
            if command == "preset_load" || command == "preset_load_tab" {
                GrandfatherKeying::LegacyPresetModeIndex
            } else {
                GrandfatherKeying::StableId
            },
        )
        .map_err(wire_error)?;
        let options = EditorPatchCommitOptions {
            mutation_id: uuid::Uuid::new_v4().to_string(),
            gesture_id: None,
            gesture_ids: Vec::new(),
            origin: EditorCommitOrigin::LegacyAdapter(command.into()),
            record_history,
            apply_key_side_effects: true,
            enforce_touched_fields: false,
        };
        let mut prepared = prepare_editor_commit(
            &self.history,
            0,
            before,
            current,
            candidate,
            self.store.clone(),
            fields,
            None,
            options,
        )
        .map_err(wire_error)?;
        if let Some(store) = prepared.take_pending_store() {
            self.store = store;
        }
        let change = prepared.finalize(&mut self.history, false, 0);
        self.publish_editor(out, change.event, &change.result.changed_fields, legacy)?;
        if let Some(status) = change.history_status {
            out.event("history:status", status)?;
        }
        Ok(())
    }
    fn preset_command(&mut self, command: &str, args: &Value) -> Result<Output, String> {
        let original = self.store.clone();
        let before = PresetFullHistorySnapshot::from_store(&original);
        // wire 필드 검증과 과거 프리셋 마이그레이션도 앱과 동일한 진입점 사용
        let preset =
            crate::preset::validation::decode_preset(&encode(&arg::<Value>(args, "preset")?)?)?;
        let prepared = crate::web_preset::prepare_import(
            &self.store,
            preset,
            command == "preset_load_tab",
            &optional::<WebAssetMap>(args, "assets")?.unwrap_or_default(),
        )?;
        self.store = prepared.store;
        let history = (!before.matches_store(&self.store))
            .then(|| self.history.prepare_preset_full_entry(before))
            .transpose()?;
        let mut out = Output {
            result: json!({"success":true}),
            asset_writes: prepared.asset_writes,
            ..Default::default()
        };
        self.commit_resource_editor(original.clone(), command, false, false, &mut out)?;
        let history_changed = history.is_some();
        if let Some(plan) = history {
            self.history.apply_record_plan(plan);
        }
        if let Some(mut diff) = prepared.settings_diff {
            diff.full = None;
            out.event("settings:changed", diff)?;
        }
        out.event("layerGroups:changed", &self.store.layer_groups)?;
        if command == "preset_load" {
            self.preset_snapshot_event(&mut out)?;
        } else {
            self.publish_editor(
                &mut out,
                None,
                &[
                    EditorField::Keys,
                    EditorField::KeyPositions,
                    EditorField::StatPositions,
                    EditorField::GraphPositions,
                    EditorField::KnobPositions,
                    EditorField::SpritePositions,
                ],
                true,
            )?;
            out.event("tabNote:changed_all", &self.store.tab_note_overrides)?;
        }
        if let Some(publication) = prepared.publication {
            out.event("css:use", json!({"enabled":publication.use_custom_css}))?;
            out.event("css:content", publication.custom_css)?;
            out.event("js:use", json!({"enabled":publication.use_custom_js}))?;
            out.event("js:content", publication.custom_js)?;
        }
        for (id, css) in &self.store.tab_css_overrides {
            out.event("tabCss:changed", json!({"tabId":id,"css":css}))?;
        }
        for id in original
            .tab_css_overrides
            .keys()
            .filter(|id| !self.store.tab_css_overrides.contains_key(*id))
        {
            out.event("tabCss:changed", json!({"tabId":id,"css":null}))?;
        }
        if original.key_counters != self.store.key_counters {
            self.counter_event(&mut out)?;
        }
        if history_changed {
            self.history_event(&mut out)?;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
