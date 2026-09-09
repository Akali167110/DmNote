use super::*;
use crate::defaults::{default_keys, default_positions, default_stat_positions};
use crate::state::tab_data::*;
use crate::state::tab_metadata::{
    normalize_bar_count, normalize_tab_order, validate_custom_tab_name,
};

impl SessionState {
    pub(super) fn tabs_command(&mut self, command: &str, args: &Value) -> Result<Output, String> {
        self.observed_epoch(args)?;
        let before = self.store.clone();
        let mut out = Output::default();
        let counters = command.contains("counter");
        let mode_only = matches!(command, "keys_set_mode" | "custom_tabs_select");
        let reset = matches!(
            command,
            "keys_reset_all" | "keys_reset_mode" | "custom_tabs_delete"
        );
        let mut reset_mode = None;
        let mut changed_plugins = Vec::new();
        let mut selection_authoritative = !matches!(command, "custom_tabs_rename" | "tabs_reorder");
        match command {
            "keys_set_mode" | "custom_tabs_select" => {
                let requested: String = arg(
                    args,
                    if command == "keys_set_mode" {
                        "mode"
                    } else {
                        "id"
                    },
                )?;
                let (success, selected) = select_mode_if_available(&mut self.store, &requested);
                out.result = if command == "keys_set_mode" {
                    json!({"success":success,"mode":selected})
                } else if success {
                    json!({"success":true,"selected":selected})
                } else {
                    json!({"success":false,"selected":selected,"error":"not-found"})
                };
                if success {
                    out.event("keys:mode-changed", json!({"mode":selected}))?;
                }
            }
            "custom_tabs_create" => {
                let name: String = arg(args, "name")?;
                let name = match validate_custom_tab_name(&name, &self.store.custom_tabs, None) {
                    Ok(name) => name,
                    Err(error) => {
                        out.result = json!({"error":error});
                        return Ok(out);
                    }
                };
                if self.store.custom_tabs.len() >= 30 {
                    out.result = json!({"error":"max-reached"});
                    return Ok(out);
                }
                let id = format!("custom-{}", uuid::Uuid::new_v4());
                let tab = CustomTab {
                    id: id.clone(),
                    name,
                };
                self.store.custom_tabs.push(tab.clone());
                self.store.tab_order =
                    normalize_tab_order(&self.store.tab_order, &self.store.custom_tabs);
                self.store.keys.insert(id.clone(), Vec::new());
                self.store.key_positions.insert(id.clone(), Vec::new());
                self.store.selected_key_type = id;
                out.result = json!({"result":tab});
            }
            "custom_tabs_rename" => {
                let id: String = arg(args, "id")?;
                let name: String = arg(args, "name")?;
                let error = if let Some(index) =
                    self.store.custom_tabs.iter().position(|tab| tab.id == id)
                {
                    match validate_custom_tab_name(&name, &self.store.custom_tabs, Some(&id)) {
                        Ok(name) => {
                            self.store.custom_tabs[index].name = name;
                            None
                        }
                        Err(error) => Some(error.to_string()),
                    }
                } else {
                    Some("unknown-tab".into())
                };
                out.result = json!({"result":self.tab_metadata()});
                if let Some(error) = error {
                    out.result["error"] = json!(error);
                }
            }
            "tabs_reorder" => {
                #[derive(serde::Deserialize)]
                #[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
                enum Op {
                    Swap { a: String, b: String },
                }
                let Op::Swap { a, b } = arg(args, "op")?;
                let mut order = normalize_tab_order(&self.store.tab_order, &self.store.custom_tabs);
                let error = match (
                    order.iter().position(|id| id == &a),
                    order.iter().position(|id| id == &b),
                ) {
                    (Some(a), Some(b)) => {
                        order.swap(a, b);
                        None
                    }
                    _ => Some("unknown-tab"),
                };
                self.store.bar_count = normalize_bar_count(self.store.bar_count, &order);
                self.store.tab_order = order;
                out.result = json!({"result":self.tab_metadata()});
                if let Some(error) = error {
                    out.result["error"] = json!(error);
                }
            }
            "custom_tabs_restore" => {
                let tabs: Vec<CustomTab> = arg(args, "customTabs")?;
                let selected: String = arg(args, "selectedKeyType")?;
                let order = normalize_tab_order(&self.store.tab_order, &tabs);
                validate_history_restore_metadata(
                    &EditorDocumentV1::from_store(&self.store),
                    &tabs,
                    &order,
                    &selected,
                )
                .map_err(wire_error)?;
                self.store.custom_tabs = tabs;
                self.store.tab_order = order;
                self.store.bar_count =
                    normalize_bar_count(self.store.bar_count, &self.store.tab_order);
                self.store.selected_key_type = selected;
            }
            "custom_tabs_delete" => {
                let id: String = arg(args, "id")?;
                let Some(plan) = plan_custom_tab_delete(&self.store, &id) else {
                    out.result = json!({"success":false,"selected":self.store.selected_key_type,"error":"not-found"});
                    return Ok(out);
                };
                delete_custom_tab_data(&mut self.store, &id, &plan);
                reset_mode = Some(id);
                out.result = json!({"success":true,"selected":self.store.selected_key_type});
            }
            "keys_reset_all" => {
                reset_all_editor_data(
                    &mut self.store,
                    default_keys(),
                    default_positions(),
                    default_stat_positions(),
                );
                let defaults = SettingsState::default();
                let note = NoteSettings::default();
                let patch = SettingsPatchInput {
                    background_color: Some(defaults.background_color),
                    laboratory_enabled: Some(defaults.laboratory_enabled),
                    use_custom_css: Some(defaults.use_custom_css),
                    custom_css: Some(CustomCssPatch {
                        path: Some(defaults.custom_css.path),
                        content: Some(defaults.custom_css.content),
                    }),
                    note_effect: Some(defaults.note_effect),
                    overlay_locked: Some(defaults.overlay_locked),
                    note_settings: Some(NoteSettingsPatch {
                        frame_limit: Some(note.frame_limit),
                        speed: Some(note.speed),
                        track_height: Some(note.track_height),
                        fade_position: Some(note.fade_position),
                        fade_top_px: Some(note.fade_top_px),
                        fade_bottom_px: Some(note.fade_bottom_px),
                        reverse: Some(note.reverse),
                        reverse_fade_top_px: Some(note.reverse_fade_top_px),
                        reverse_fade_bottom_px: Some(note.reverse_fade_bottom_px),
                        delayed_note_enabled: Some(note.delayed_note_enabled),
                        short_note_threshold_ms: Some(note.short_note_threshold_ms),
                        short_note_min_length_px: Some(note.short_note_min_length_px),
                        key_display_delay_ms: Some(note.key_display_delay_ms),
                    }),
                    ..Default::default()
                };
                let mut diff = crate::settings::apply_patch_to_store(&mut self.store, &patch);
                diff.full = None;
                out.event("settings:changed", diff)?;
                out.event("css:use", json!({"enabled":false}))?;
                out.event("css:content", json!({"path":null,"content":""}))?;
                out.result = self.tab_metadata();
                out.result["keys"] = value(&self.store.keys)?;
                out.result["positions"] = value(&self.store.key_positions)?;
            }
            "keys_reset_mode" => {
                let mode: String = arg(args, "mode")?;
                let Some(kind) = reset_mode_kind(&self.store, &mode) else {
                    out.result = json!({"success":false,"mode":mode});
                    return Ok(out);
                };
                reset_mode_data(&mut self.store, &mode, kind);
                reset_mode = Some(mode.clone());
                selection_authoritative = false;
                out.result = json!({"success":true,"mode":mode});
            }
            "keys_set_counters" => {
                self.store.key_counters = arg(args, "counters")?;
                sync_key_counters(&mut self.store.key_counters, &self.store.keys);
            }
            "keys_reset_counters" => {
                for mode in self.store.key_counters.values_mut() {
                    for count in mode.values_mut() {
                        *count = 0;
                    }
                }
            }
            "keys_reset_counters_mode" | "keys_reset_single_counter" => {
                let mode: String = arg(args, "mode")?;
                let key: Option<String> = if command == "keys_reset_single_counter" {
                    Some(arg(args, "key")?)
                } else {
                    None
                };
                if let Some(values) = self.store.key_counters.get_mut(&mode) {
                    for (label, count) in values {
                        if key.as_ref().is_none_or(|key| key == label) {
                            *count = 0;
                        }
                    }
                }
            }
            _ => return Err(format!("UNSUPPORTED_WEB_COMMAND:{command}")),
        }
        if reset {
            changed_plugins = reset_instances(&mut self.store, reset_mode.as_deref())?;
            if !changed_plugins.is_empty() {
                self.plugin_revision = next_plugin_model_revision(self.plugin_revision)?;
            }
        }
        if command == "keys_reset_all" || command == "keys_reset_mode" {
            out.asset_writes
                .extend(crate::web_preset::restore_inline_store_images(
                    &mut self.store,
                    &optional(args, "assets")?.unwrap_or_default(),
                )?);
        }
        crate::state::migration::canonicalize_gradient_pairs(&mut self.store);
        let current = EditorDocumentV1::from_store(&before);
        let candidate = EditorDocumentV1::from_store(&self.store);
        validate_document_transition_with_keying(
            &current,
            &candidate,
            &before,
            &self.store,
            if command.starts_with("keys_reset_") {
                GrandfatherKeying::LegacyPresetModeIndex
            } else {
                GrandfatherKeying::StableId
            },
        )
        .map_err(wire_error)?;
        let changed_fields = current.changed_fields(&candidate);
        if changed_fields.contains(&EditorField::Keys) {
            sync_key_counters(&mut self.store.key_counters, &candidate.keys);
        }
        let plan = if counters {
            if before.key_counters != self.store.key_counters {
                Some(
                    self.history
                        .prepare_counters_entry(before.key_counters.clone())?,
                )
            } else {
                None
            }
        } else if mode_only {
            if before.selected_key_type != self.store.selected_key_type {
                Some(
                    self.history
                        .prepare_mode_entry(before.selected_key_type.clone())?,
                )
            } else {
                None
            }
        } else if command.starts_with("keys_reset_") {
            None
        } else {
            let snapshot = CustomTabsHistorySnapshot::from_transition(&before, &self.store);
            if !snapshot.matches_store(&self.store) {
                Some(self.history.prepare_custom_tabs_entry(snapshot)?)
            } else {
                None
            }
        };
        if !changed_fields.is_empty() {
            self.store.editor_revision =
                next_revision(before.editor_revision).map_err(wire_error)?;
            let event = EditorCommittedV1 {
                schema_version: EDITOR_SCHEMA_VERSION,
                revision: self.store.editor_revision,
                mutation_id: uuid::Uuid::new_v4().to_string(),
                gesture_id: None,
                gesture_ids: Vec::new(),
                origin: EditorCommitOrigin::LegacyAdapter(command.into())
                    .event_name()
                    .unwrap(),
                changed_fields: changed_fields.clone(),
                patch: candidate.patch_for_fields(&changed_fields),
            };
            self.publish_editor(&mut out, Some(event), &changed_fields, true)?;
        }
        let history_changed = plan.is_some();
        if let Some(plan) = plan {
            self.history.apply_record_plan(plan);
        }
        if reset {
            if command.starts_with("keys_reset_") && before != self.store {
                self.history.invalidate_all();
            }
            self.history.advance_epoch();
        }
        if history_changed || reset {
            self.history_event(&mut out)?;
        }
        if before.key_counters != self.store.key_counters {
            self.counter_event(&mut out)?;
        }
        if counters {
            out.result = value(&self.store.key_counters)?;
        }
        if !mode_only && !counters && (before != self.store || command == "custom_tabs_restore") {
            let mut payload = self.tab_metadata();
            payload["selectionAuthoritative"] = json!(selection_authoritative);
            out.event("customTabs:changed", payload)?;
            if selection_authoritative {
                out.event(
                    "keys:mode-changed",
                    json!({"mode":self.store.selected_key_type}),
                )?;
            }
        }
        if reset {
            out.event("tabNote:changed_all", &self.store.tab_note_overrides)?;
            for id in before
                .tab_css_overrides
                .keys()
                .filter(|id| !self.store.tab_css_overrides.contains_key(*id))
            {
                out.event("tabCss:changed", json!({"tabId":id,"css":null}))?;
            }
        }
        for plugin_id in changed_plugins {
            out.event(
                "pluginInstances:changed",
                PluginInstancesChangedPayload {
                    plugin_id,
                    revision: self.plugin_revision,
                    origin_mutation_id: None,
                },
            )?;
        }
        Ok(out)
    }
    fn tab_metadata(&self) -> Value {
        json!({"customTabs":self.store.custom_tabs,"tabOrder":self.store.tab_order,"barCount":self.store.bar_count,"selectedKeyType":self.store.selected_key_type})
    }
}

fn reset_instances(store: &mut AppStoreData, mode: Option<&str>) -> Result<Vec<String>, String> {
    let mut changed = Vec::new();
    for plugin_id in collect_plugin_instance_ids(store.plugin_data.keys().map(String::as_str)) {
        let key = plugin_instances_storage_key(&plugin_id);
        if let Some(mode) = mode {
            let Some(mut entries) = store
                .plugin_data
                .get(&key)
                .and_then(|value| decode_plugin_instance_entries(value, &key))
            else {
                continue;
            };
            let count = entries.len();
            entries.retain(|entry| match entry {
                StoredPluginInstanceEntry::Parsed { instance, .. } => {
                    normalize_plugin_instance_tab_id(instance.tab_id.as_deref()) != mode
                }
                _ => true,
            });
            if entries.len() == count {
                continue;
            }
            if entries.is_empty() {
                store.plugin_data.remove(&key);
            } else {
                store
                    .plugin_data
                    .insert(key.clone(), encode_plugin_instance_entries(entries, &key));
            }
        } else {
            store.plugin_data.remove(&key);
        }
        changed.push(plugin_id);
    }
    Ok(changed)
}
