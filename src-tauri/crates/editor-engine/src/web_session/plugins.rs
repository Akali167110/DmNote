use super::*;
use std::collections::HashSet;
impl SessionState {
    pub(super) fn plugin_command(&mut self, command: &str, args: &Value) -> Result<Output, String> {
        let mut out = Output::default();
        match command {
            "plugin_authority_reset" => {
                self.authority_generation =
                    next_revision(self.authority_generation).map_err(wire_error)?;
                self.authority_available = true;
                out.result = value(PluginAuthoritySnapshot {
                    authority_generation: self.authority_generation,
                    model_revision: self.plugin_revision,
                })?;
            }
            "plugin_instances_commit" | "plugin_instances_reconcile" => {
                let (id, instances, gesture, mutation) = if command == "plugin_instances_commit" {
                    let request: PluginInstancesCommitRequest = arg(args, "request")?;
                    validate_plugin_instances_request(&request)?;
                    self.authority(request.authority_generation)?;
                    if request
                        .observed_history_epoch
                        .is_some_and(|epoch| epoch != self.history.history_epoch())
                    {
                        return Err("HISTORY_EPOCH_CONFLICT".into());
                    }
                    if request
                        .expected_model_revision
                        .is_some_and(|rev| rev != self.plugin_revision)
                    {
                        return Err("PLUGIN_MODEL_REVISION_CONFLICT".into());
                    }
                    (
                        request.plugin_id,
                        request.instances,
                        request.gesture_id,
                        request.mutation_id,
                    )
                } else {
                    let request: PluginInstancesReconcileRequest = arg(args, "request")?;
                    validate_plugin_instances_reconcile_request(&request)?;
                    self.authority(request.authority_generation)?;
                    if request
                        .observed_history_epoch
                        .is_some_and(|epoch| epoch != self.history.history_epoch())
                    {
                        return Err("HISTORY_EPOCH_CONFLICT".into());
                    }
                    let mut instances = plugin_elements_snapshot(&self.store, &request.plugin_id)?
                        .instances
                        .unwrap_or_default();
                    let valid = request.valid_tab_ids.into_iter().collect::<HashSet<_>>();
                    instances.retain_mut(|instance| {
                        let tab = normalize_plugin_instance_tab_id(instance.tab_id.as_deref())
                            .to_string();
                        if !valid.contains(&tab) {
                            return false;
                        }
                        instance.tab_id = Some(tab);
                        true
                    });
                    (request.plugin_id, instances, None, request.mutation_id)
                };
                let before = plugin_elements_snapshot(&self.store, &id)?;
                validate_plugin_instances_transition(
                    before.instances.as_deref().unwrap_or_default(),
                    &instances,
                )?;
                let changed = before.instances.as_deref().unwrap_or_default() != instances;
                if changed {
                    let plan = self
                        .history
                        .prepare_plugin_elements_entry(before, gesture)?;
                    let canonical = PluginElementsHistorySnapshot {
                        plugin_id: id.clone(),
                        instances: (!instances.is_empty()).then_some(instances),
                    };
                    apply_plugin_elements_snapshot(&mut self.store, &canonical)?;
                    self.plugin_revision = next_plugin_model_revision(self.plugin_revision)?;
                    self.history
                        .apply_plugin_elements_record_plan(plan, &canonical);
                    out.event(
                        "pluginInstances:changed",
                        PluginInstancesChangedPayload {
                            plugin_id: id.clone(),
                            revision: self.plugin_revision,
                            origin_mutation_id: Some(mutation),
                        },
                    )?;
                    self.history_event(&mut out)?;
                }
                out.result = value(PluginInstancesCommitResult {
                    plugin_id: id,
                    model_revision: self.plugin_revision,
                    authority_generation: self.authority_generation,
                    changed,
                })?;
            }
            "plugin_storage_set" | "plugin_storage_remove" => {
                let key = format!("{PLUGIN_DATA_KEY_PREFIX}{}", arg::<String>(args, "key")?);
                if is_plugin_instances_storage_key(&key) {
                    return Err("PLUGIN_INSTANCES_KEY_RESERVED".into());
                }
                if command == "plugin_storage_set" {
                    self.store.plugin_data.insert(key, arg(args, "value")?);
                } else {
                    self.store.plugin_data.remove(&key);
                }
            }
            "plugin_storage_clear" | "plugin_storage_clear_by_prefix" => {
                let prefix = if command == "plugin_storage_clear" {
                    None
                } else {
                    Some(format!(
                        "{PLUGIN_DATA_KEY_PREFIX}{}",
                        arg::<String>(args, "prefix")?
                    ))
                };
                let namespace = prefix
                    .as_deref()
                    .and_then(plugin_id_from_storage_namespace_prefix);
                let canonical = namespace.map(plugin_instances_storage_key);
                let history_exists = if prefix.is_none() {
                    self.history.contains_plugin_elements_for(None)
                } else {
                    namespace.is_some_and(|id| self.history.contains_plugin_elements_for(Some(id)))
                };
                let keys: Vec<_> = self
                    .store
                    .plugin_data
                    .keys()
                    .filter(|key| {
                        prefix.as_ref().is_none_or(|prefix| {
                            key.starts_with(prefix) || canonical.as_ref() == Some(key)
                        })
                    })
                    .cloned()
                    .collect();
                let ids = collect_plugin_instance_ids(keys.iter().map(String::as_str));
                if !ids.is_empty() {
                    self.plugin_revision = next_plugin_model_revision(self.plugin_revision)?;
                }
                for key in &keys {
                    self.store.plugin_data.remove(key);
                }
                if (!ids.is_empty() || history_exists) && self.history.invalidate_all() {
                    self.history_event(&mut out)?;
                }
                for plugin_id in ids {
                    out.event(
                        "pluginInstances:changed",
                        PluginInstancesChangedPayload {
                            plugin_id,
                            revision: self.plugin_revision,
                            origin_mutation_id: None,
                        },
                    )?;
                }
                if prefix.is_some() {
                    out.result = value(keys.len())?;
                }
            }
            _ => return Err(format!("UNSUPPORTED_WEB_COMMAND:{command}")),
        }
        Ok(out)
    }
}
