use super::*;
use crate::web_resources::counter_animation::*;
impl SessionState {
    pub(super) fn counter_command(
        &mut self,
        command: &str,
        args: &Value,
    ) -> Result<Output, String> {
        let before = self.store.clone();
        let mut out = Output::default();
        let affected;
        match command {
            "counter_animation_create" | "counter_animation_update" => {
                let request: Value = arg(args, "request")?;
                let id = if command == "counter_animation_create" {
                    format!("user-{}", uuid::Uuid::new_v4().simple())
                } else {
                    arg::<String>(&request, "id")?.trim().to_string()
                };
                if id.is_empty() {
                    return Err("counter animation id is required".into());
                }
                let mut preset = CounterAnimationPreset {
                    id: id.clone(),
                    name: arg(&request, "name")?,
                    source: CounterAnimationSource::User,
                    label_key: None,
                    bezier: arg(&request, "bezier")?,
                    scale: arg(&request, "scale")?,
                    duration_ms: arg(&request, "durationMs")?,
                };
                preset.normalize();
                if preset.name.is_empty() {
                    return Err("counter animation name cannot be empty".into());
                }
                if command == "counter_animation_create" {
                    self.store.counter_animation_presets.push(preset.clone());
                    affected = 0;
                } else {
                    if !self
                        .store
                        .counter_animation_presets
                        .iter()
                        .any(|preset| preset.id == id)
                    {
                        return Err(format!("counter animation preset not found: {id}"));
                    }
                    replace_counter_animation_preset(&mut self.store, &id, &preset)
                        .map_err(wire_error)?;
                    affected = apply_preset_to_bound_counters(&mut self.store, &id, &preset);
                }
                out.result = json!({"preset":preset,"affectedUsageCount":affected});
            }
            "counter_animation_delete" => {
                let id = arg::<String>(args, "id")?.trim().to_string();
                if id.is_empty() {
                    return Err("counter animation id is required".into());
                }
                let fallback = find_builtin_counter_animation_preset_by_id(
                    default_counter_animation_preset_id(),
                )
                .ok_or("default builtin counter animation preset missing")?;
                if !self
                    .store
                    .counter_animation_presets
                    .iter()
                    .any(|preset| preset.id == id)
                {
                    return Err(format!("counter animation preset not found: {id}"));
                }
                remove_counter_animation_preset(&mut self.store, &id).map_err(wire_error)?;
                affected = apply_fallback_to_bound_counters(&mut self.store, &id, &fallback);
                out.result = json!({"success":true,"id":id,"affectedUsageCount":affected,"fallbackPresetId":fallback.id});
            }
            _ => return Err(format!("UNSUPPORTED_WEB_COMMAND:{command}")),
        }
        let unchanged =
            EditorDocumentV1::from_store(&before) == EditorDocumentV1::from_store(&self.store);
        if command != "counter_animation_create" {
            self.commit_resource_editor(
                before,
                command,
                command != "counter_animation_delete",
                true,
                &mut out,
            )?;
            if command == "counter_animation_delete" && self.history.invalidate_all() {
                self.history_event(&mut out)?;
            }
        }
        out.event("counterAnimation:changed",json!({"builtinPresets":default_counter_animation_builtin_presets(),"userPresets":self.store.counter_animation_presets}))?;
        if affected > 0 && unchanged {
            out.event("positions:changed", &self.store.key_positions)?;
            out.event("statPositions:changed", &self.store.stat_positions)?;
            out.event("graphPositions:changed", &self.store.graph_positions)?;
        }
        Ok(out)
    }
}
