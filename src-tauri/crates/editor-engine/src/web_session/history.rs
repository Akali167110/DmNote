use super::*;
use crate::history_transition::{prepare_history_operation, HistoryAuxChange};
impl SessionState {
    pub(super) fn restore_history(
        &mut self,
        command: &str,
        args: &Value,
    ) -> Result<Output, String> {
        let id: String = arg(args, "operationId")?;
        let counters = self.store.key_counters.clone();
        let prepared = prepare_history_operation(
            &self.store,
            &self.history,
            self.plugin_revision,
            if command == "history_undo" {
                HistoryDirection::Undo
            } else {
                HistoryDirection::Redo
            },
            &id,
            &counters,
        );
        let result = prepared.outcome?;
        self.store = prepared.store;
        self.history = prepared.history;
        self.plugin_revision = prepared.plugin_revision;
        let mut out = Output {
            result: value(&result.status)?,
            ..Default::default()
        };
        if result.replayed {
            return Ok(out);
        }
        if let Some(change) = result.change {
            self.publish_editor(&mut out, change.event, &change.result.changed_fields, true)?;
        }
        match result.aux_change {
            Some(HistoryAuxChange::CustomTabs {
                changed_tab_css_ids,
                plugin_ids,
                revision,
                ..
            }) => {
                self.history_tabs_events(&mut out)?;
                for id in changed_tab_css_ids {
                    out.event(
                        "tabCss:changed",
                        json!({"tabId":id,"css":self.store.tab_css_overrides.get(&id)}),
                    )?;
                }
                for plugin_id in plugin_ids {
                    out.event(
                        "pluginInstances:changed",
                        PluginInstancesChangedPayload {
                            plugin_id,
                            revision,
                            origin_mutation_id: None,
                        },
                    )?;
                }
            }
            Some(HistoryAuxChange::PresetFull {
                snapshot,
                mut settings_diff,
                changed_tab_css_ids,
            }) => {
                settings_diff.full = None;
                out.event("settings:changed", settings_diff)?;
                self.history_tabs_events(&mut out)?;
                out.event("layerGroups:changed", &self.store.layer_groups)?;
                self.preset_snapshot_event(&mut out)?;
                for id in changed_tab_css_ids {
                    out.event(
                        "tabCss:changed",
                        json!({"tabId":id,"css":self.store.tab_css_overrides.get(&id)}),
                    )?;
                }
                out.event(
                    "css:use",
                    json!({"enabled":snapshot.settings.use_custom_css}),
                )?;
                out.event("css:content", &snapshot.settings.custom_css)?;
                out.event("js:use", json!({"enabled":snapshot.settings.use_custom_js}))?;
                out.event("js:content", &self.store.custom_js)?;
            }
            Some(HistoryAuxChange::Mode(mode)) => {
                out.event("keys:mode-changed", json!({"mode":mode}))?
            }
            Some(HistoryAuxChange::Counters(_)) => {}
            Some(HistoryAuxChange::PluginElements {
                plugin_id,
                revision,
            }) => out.event(
                "pluginInstances:changed",
                PluginInstancesChangedPayload {
                    plugin_id,
                    revision,
                    origin_mutation_id: None,
                },
            )?,
            Some(HistoryAuxChange::PluginElementsBatch {
                plugin_ids,
                revision,
            }) => {
                for plugin_id in plugin_ids {
                    out.event(
                        "pluginInstances:changed",
                        PluginInstancesChangedPayload {
                            plugin_id,
                            revision,
                            origin_mutation_id: None,
                        },
                    )?;
                }
            }
            None => {}
        }
        if counters != self.store.key_counters {
            self.counter_event(&mut out)?;
        }
        let status = self.history.issue_status(false);
        out.result = value(&status)?;
        out.event("history:status", status)?;
        Ok(out)
    }
    pub(super) fn counter_event(&mut self, out: &mut Output) -> Result<(), String> {
        self.counters_revision = self.counters_revision.saturating_add(1);
        out.event("keys:counters", &self.store.key_counters)?;
        out.event("keys:counters-state",json!({"sessionId":self.counters_session_id,"revision":self.counters_revision,"counters":self.store.key_counters}))
    }
    pub(super) fn history_tabs_events(&self, out: &mut Output) -> Result<(), String> {
        out.event("customTabs:changed",json!({"customTabs":self.store.custom_tabs,"tabOrder":self.store.tab_order,"barCount":self.store.bar_count,"selectedKeyType":self.store.selected_key_type,"selectionAuthoritative":true}))?;
        out.event(
            "keys:mode-changed",
            json!({"mode":self.store.selected_key_type}),
        )?;
        out.event("tabNote:changed_all", &self.store.tab_note_overrides)
    }
    pub(super) fn preset_snapshot_event(&self, out: &mut Output) -> Result<(), String> {
        out.event("preset:snapshot",json!({"keys":self.store.keys,"positions":self.store.key_positions,"statPositions":self.store.stat_positions,"graphPositions":self.store.graph_positions,"knobPositions":self.store.knob_positions,"spritePositions":self.store.sprite_positions,"customTabs":self.store.custom_tabs,"tabOrder":self.store.tab_order,"barCount":self.store.bar_count,"selectedKeyType":self.store.selected_key_type,"tabNoteOverrides":self.store.tab_note_overrides}))
    }
}
