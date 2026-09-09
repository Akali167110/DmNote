use super::*;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
impl SessionState {
    pub(super) fn js_command(&mut self, command: &str, args: &Value) -> Result<Output, String> {
        let mut out = Output::default();
        let mut publish = true;
        let mut forced = false;
        if matches!(
            command,
            "js_set_content" | "js_reload" | "js_remove_plugin" | "js_set_plugin_enabled"
        ) {
            self.store.custom_js.normalize();
        }
        match command {
            "js_toggle" => {
                let enabled = arg(args, "enabled")?;
                self.store.use_custom_js = enabled;
                if enabled {
                    self.store.custom_js.normalize();
                }
                out.result = json!({"enabled":enabled});
                out.event("js:use", out.result.clone())?;
                publish = enabled;
            }
            "js_reset" => {
                self.store.use_custom_js = false;
                self.store.custom_js = CustomJs::default();
                out.event("js:use", json!({"enabled":false}))?;
            }
            "js_set_content" => {
                let content: String = arg(args, "content")?;
                let script = &mut self.store.custom_js;
                if script.plugins.is_empty() {
                    script.content = content;
                } else if let Some(plugin) = script.plugins.iter_mut().find(|plugin| plugin.enabled)
                {
                    plugin.content = content;
                } else if let Some(plugin) = script.plugins.first_mut() {
                    plugin.content = content;
                }
                script.normalize();
                out.result = json!({"success":true});
            }
            "js_load" => {
                let files: Vec<WebFile> = optional(args, "files")?.unwrap_or_default();
                let mut added = Vec::new();
                let mut errors = Vec::new();
                for file in files {
                    let path = format!("/assets/scripts/{}.js", uuid::Uuid::new_v4());
                    let content = BASE64_STANDARD
                        .decode(&file.data_base64)
                        .map_err(|e| e.to_string())
                        .and_then(|bytes| String::from_utf8(bytes).map_err(|e| e.to_string()));
                    match content {
                        Ok(content) => {
                            out.asset_writes.insert(path.clone(), file.data_base64);
                            added.push(JsPlugin {
                                id: uuid::Uuid::new_v4().to_string(),
                                name: file.name,
                                path: Some(path),
                                content,
                                enabled: true,
                            });
                        }
                        Err(error) => errors.push(json!({"path":file.name,"error":error})),
                    }
                }
                out.result = json!({"success":!added.is_empty()});
                if !errors.is_empty() {
                    out.result["errors"] = json!(errors);
                }
                if added.is_empty() {
                    publish = false;
                } else {
                    out.result["added"] = value(&added)?;
                    let script = &mut self.store.custom_js;
                    script.normalize();
                    script.plugins.extend(added);
                    script.path = None;
                    script.content.clear();
                    script.normalize();
                }
            }
            "js_reload" => {
                let assets: WebAssetMap = optional(args, "assets")?.unwrap_or_default();
                let mut updated = Vec::new();
                let mut errors = Vec::new();
                for plugin in &mut self.store.custom_js.plugins {
                    let Some(path) = &plugin.path else {
                        continue;
                    };
                    let content = assets
                        .get(path)
                        .ok_or_else(|| "source-not-found".to_string())
                        .and_then(|data| BASE64_STANDARD.decode(data).map_err(|e| e.to_string()))
                        .and_then(|bytes| String::from_utf8(bytes).map_err(|e| e.to_string()));
                    match content {
                        Ok(content) => {
                            plugin.content = content;
                            updated.push(plugin.clone());
                        }
                        Err(error) => errors.push(json!({"path":path,"error":error})),
                    }
                }
                self.store.custom_js.normalize();
                forced = true;
                out.result = json!({});
                if !updated.is_empty() {
                    out.result["updated"] = value(updated)?;
                }
                if !errors.is_empty() {
                    out.result["errors"] = json!(errors);
                }
            }
            "js_remove_plugin" => {
                let id: String = arg(args, "id")?;
                let script = &mut self.store.custom_js;
                let before = script.plugins.len();
                script.plugins.retain(|plugin| plugin.id != id);
                let removed = script.plugins.len() != before;
                script.normalize();
                publish = removed;
                out.result = if removed {
                    json!({"success":true,"removed_id":id})
                } else {
                    json!({"success":false,"error":"not-found"})
                };
            }
            "js_set_plugin_enabled" => {
                let id: String = arg(args, "id")?;
                let enabled = arg(args, "enabled")?;
                let plugin = self
                    .store
                    .custom_js
                    .plugins
                    .iter_mut()
                    .find(|plugin| plugin.id == id)
                    .map(|plugin| {
                        plugin.enabled = enabled;
                        plugin.clone()
                    });
                self.store.custom_js.normalize();
                publish = plugin.is_some();
                out.result = if let Some(plugin) = plugin {
                    json!({"success":true,"plugin":plugin})
                } else {
                    json!({"success":false,"error":"not-found"})
                };
            }
            _ => return Err(format!("UNSUPPORTED_WEB_COMMAND:{command}")),
        }
        if publish {
            let mut payload = value(&self.store.custom_js)?;
            payload["forced"] = json!(forced);
            out.event("js:content", payload)?;
        }
        Ok(out)
    }
}
