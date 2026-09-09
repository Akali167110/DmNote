use super::*;
use std::collections::BTreeMap;
impl SessionState {
    pub(super) fn read(&self, command: &str, args: &Value) -> Result<Value, String> {
        match command {
            "app_bootstrap" => value(BootstrapPayload {
                settings: self.store.settings_state(),
                defaults: DefaultsPayload {
                    settings: SettingsState::default(),
                    counter_settings: KeyCounterSettings::default(),
                },
                keys: self.store.keys.clone(),
                positions: self.store.key_positions.clone(),
                stat_positions: self.store.stat_positions.clone(),
                graph_positions: self.store.graph_positions.clone(),
                knob_positions: self.store.knob_positions.clone(),
                sprite_positions: self.store.sprite_positions.clone(),
                custom_tabs: self.store.custom_tabs.clone(),
                tab_order: self.store.tab_order.clone(),
                bar_count: self.store.bar_count,
                selected_key_type: self.store.selected_key_type.clone(),
                current_mode: self.store.selected_key_type.clone(),
                active_keys: Vec::new(),
                overlay: self.overlay(),
                key_counters: self.store.key_counters.clone(),
                key_counters_session_id: self.counters_session_id.clone(),
                key_counters_revision: self.counters_revision,
                layer_groups: self.store.layer_groups.clone(),
                tab_note_overrides: self.store.tab_note_overrides.clone(),
                tab_css_overrides: self.store.tab_css_overrides.clone(),
                editor_revision: self.store.editor_revision,
            }),
            "editor_get" => value(EditorGetResult {
                revision: self.store.editor_revision,
                document: EditorDocumentV1::from_store(&self.store),
            }),
            "history_status" => value(self.history.status(false)),
            "settings_get" => value(self.store.settings_state()),
            "keys_get" => value(&self.store.keys),
            "positions_get" => value(&self.store.key_positions),
            "stat_positions_get" => value(&self.store.stat_positions),
            "graph_positions_get" => value(&self.store.graph_positions),
            "knob_positions_get" => value(&self.store.knob_positions),
            "sprite_positions_get" => value(&self.store.sprite_positions),
            "layer_groups_get" => value(&self.store.layer_groups),
            "keys_get_counters" => value(&self.store.key_counters),
            "custom_tabs_list" => value(&self.store.custom_tabs),
            "note_tab_get_all" => value(&self.store.tab_note_overrides),
            "note_tab_get" => {
                let id: String = arg(args, "tabId")?;
                Ok(json!({"tabId":id,"settings":self.store.tab_note_overrides.get(&id)}))
            }
            "css_tab_export" => self.css_export(args),
            "css_tab_get_all" => value(&self.store.tab_css_overrides),
            "css_tab_get" => {
                let id: String = arg(args, "tabId")?;
                Ok(json!({"tabId":id,"css":self.store.tab_css_overrides.get(&id)}))
            }
            "css_get" => value(&self.store.custom_css),
            "css_get_use" => value(self.store.use_custom_css),
            "css_history_get" => Ok(self.css_history_items(
                &optional::<crate::web_preset::WebAssetMap>(args, "assets")?.unwrap_or_default(),
            )),
            "js_get" => {
                let mut script = self.store.custom_js.clone();
                script.normalize();
                value(script)
            }
            "js_get_use" => value(self.store.use_custom_js),
            "overlay_get" => value(self.overlay()),
            "counter_animation_list" => Ok(
                json!({"builtinPresets":default_counter_animation_builtin_presets(),"userPresets":self.store.counter_animation_presets}),
            ),
            "plugin_instances_get" => {
                let id: String = arg(args, "pluginId")?;
                let snapshot = plugin_elements_snapshot(&self.store, &id)?;
                let result = PluginInstancesSnapshot {
                    plugin_id: id,
                    instances: snapshot.instances.unwrap_or_default(),
                    model_revision: self.plugin_revision,
                    authority_generation: self.authority_generation,
                };
                if encode(&result)?.len() > 8 * 1024 * 1024 {
                    return Err("PLUGIN_INSTANCES_SNAPSHOT_TOO_LARGE".into());
                }
                value(result)
            }
            "plugin_group_refs_get" => {
                let mut refs = BTreeMap::new();
                for_each_stored_plugin_instances(&self.store, |id, instances| {
                    let mut groups = PluginGroupRefs::new();
                    add_plugin_group_refs(&mut groups, &instances);
                    if !groups.is_empty() {
                        refs.insert(
                            id.to_string(),
                            groups
                                .into_iter()
                                .map(|(mode, ids)| {
                                    let mut ids: Vec<_> = ids.into_iter().collect();
                                    ids.sort();
                                    (mode, ids)
                                })
                                .collect(),
                        );
                    }
                });
                value(PluginGroupRefsSnapshot {
                    refs,
                    model_revision: self.plugin_revision,
                })
            }
            "plugin_storage_get" => Ok(self
                .store
                .plugin_data
                .get(&format!(
                    "{PLUGIN_DATA_KEY_PREFIX}{}",
                    arg::<String>(args, "key")?
                ))
                .cloned()
                .unwrap_or(Value::Null)),
            "plugin_storage_keys" => value(
                self.store
                    .plugin_data
                    .keys()
                    .filter_map(|key| key.strip_prefix(PLUGIN_DATA_KEY_PREFIX))
                    .collect::<Vec<_>>(),
            ),
            "plugin_storage_has_data" => {
                let prefix = format!("{PLUGIN_DATA_KEY_PREFIX}{}", arg::<String>(args, "prefix")?);
                let canonical = plugin_id_from_storage_namespace_prefix(&prefix)
                    .map(plugin_instances_storage_key);
                value(
                    self.store
                        .plugin_data
                        .keys()
                        .any(|key| key.starts_with(&prefix) || canonical.as_ref() == Some(key)),
                )
            }
            "preset_save" | "preset_save_tab" => value(crate::web_preset::export_preset(
                &self.store,
                command == "preset_save_tab",
                &optional(args, "assets")?.unwrap_or_default(),
            )?),
            "sound_load_original" => crate::web_resources::load_original(
                &self.store,
                &arg::<String>(args, "soundPath")?,
                &optional(args, "assets")?.unwrap_or_default(),
            ),
            _ => Err(format!("UNSUPPORTED_WEB_COMMAND:{command}")),
        }
    }
    pub(super) fn overlay(&self) -> BootstrapOverlayState {
        BootstrapOverlayState {
            visible: self.store.overlay_visible,
            locked: self.store.overlay_locked,
            anchor: self.store.overlay_resize_anchor.as_str().to_string(),
        }
    }
}
