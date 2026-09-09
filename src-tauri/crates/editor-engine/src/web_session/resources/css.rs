use super::*;
use crate::css_history::{record_custom_css_load, touch_custom_css_history};
use crate::web_resources::css::{css_content, MAX_CSS_BYTES};
impl SessionState {
    pub(super) fn css_command(&mut self, command: &str, args: &Value) -> Result<Output, String> {
        let mut out = Output::default();
        let assets: WebAssetMap = optional(args, "assets")?.unwrap_or_default();
        match command {
            "css_toggle" => {
                let enabled = arg(args, "enabled")?;
                self.store.use_custom_css = enabled;
                out.result = json!({"enabled":enabled});
                out.event("css:use", out.result.clone())?;
                if enabled {
                    out.event("css:content", &self.store.custom_css)?;
                }
            }
            "css_reset" => {
                self.store.use_custom_css = false;
                self.store.custom_css = CustomCss::default();
                out.event("css:use", json!({"enabled":false}))?;
                out.event("css:content", &self.store.custom_css)?;
            }
            "css_set_content" => {
                let content: String = arg(args, "content")?;
                if content.len() > MAX_CSS_BYTES {
                    out.result = json!({"success":false,"error":"TOO_LARGE"});
                    return Ok(out);
                }
                self.store.custom_css.content = content;
                out.result = json!({"success":true});
                out.event("css:content", &self.store.custom_css)?;
            }
            "css_load" | "css_tab_load" => {
                let tab = if command == "css_tab_load" {
                    Some(arg::<String>(args, "tabId")?)
                } else {
                    None
                };
                let files: Vec<WebFile> = optional(args, "files")?.unwrap_or_default();
                let Some(file) = files.first() else {
                    out.result = json!({"success":false});
                    if let Some(id) = tab {
                        out.result["tabId"] = json!(id);
                    }
                    return Ok(out);
                };
                if !file.name.to_ascii_lowercase().ends_with(".css") {
                    out.result = json!({"success":false,"error":"INVALID_EXTENSION"});
                    if let Some(id) = tab {
                        out.result["tabId"] = json!(id);
                    } else {
                        out.result["path"] = json!(file.name);
                    }
                    return Ok(out);
                }
                let path = format!("/assets/css/{}.css", uuid::Uuid::new_v4());
                let mut selected = assets.clone();
                selected.insert(path.clone(), file.data_base64.clone());
                let content = match css_content(&path, &selected) {
                    Ok(content) => content,
                    Err(error) => {
                        out.result = json!({"success":false,"error":error});
                        if let Some(id) = tab {
                            out.result["tabId"] = json!(id);
                        } else {
                            out.result["path"] = json!(path);
                        }
                        return Ok(out);
                    }
                };
                out.asset_writes
                    .insert(path.clone(), file.data_base64.clone());
                if let Some(id) = tab {
                    let css = TabCss {
                        path: Some(path),
                        content,
                        enabled: true,
                    };
                    self.store.tab_css_overrides.insert(id.clone(), css.clone());
                    out.result = json!({"success":true,"tabId":id,"css":css});
                    out.event("tabCss:changed", json!({"tabId":id,"css":css}))?;
                } else {
                    self.store.custom_css = CustomCss {
                        path: Some(path.clone()),
                        content: content.clone(),
                    };
                    record_custom_css_load(
                        &mut self.store.custom_css_history,
                        path.clone(),
                        arg(args, "timestampMs")?,
                    );
                    out.result = json!({"success":true,"content":content,"path":path});
                    out.event("css:content", &self.store.custom_css)?;
                }
            }
            "css_history_activate" | "css_tab_activate_history" => {
                let path: String = arg(args, "path")?;
                let tab = if command == "css_tab_activate_history" {
                    Some(arg::<String>(args, "tabId")?)
                } else {
                    None
                };
                let content = if self
                    .store
                    .custom_css_history
                    .iter()
                    .any(|entry| entry.path == path)
                {
                    css_content(&path, &assets)
                } else {
                    Err("PATH_NOT_AUTHORIZED")
                };
                let content = match content {
                    Ok(content) => content,
                    Err(code) => {
                        out.result = json!({"success":false,"code":code});
                        if let Some(id) = tab {
                            out.result["tabId"] = json!(id);
                        } else {
                            out.result["path"] = json!(path);
                        }
                        return Ok(out);
                    }
                };
                if let Some(id) = &tab {
                    if !crate::defaults::default_keys().contains_key(id)
                        && !self.store.custom_tabs.iter().any(|entry| &entry.id == id)
                    {
                        out.result = json!({"success":false,"tabId":id});
                        return Ok(out);
                    }
                }
                touch_custom_css_history(
                    &mut self.store.custom_css_history,
                    &path,
                    arg(args, "timestampMs")?,
                );
                if let Some(id) = tab {
                    let css = TabCss {
                        path: Some(path),
                        content,
                        enabled: true,
                    };
                    self.store.tab_css_overrides.insert(id.clone(), css.clone());
                    out.result = json!({"success":true,"tabId":id,"css":css});
                    out.event("tabCss:changed", json!({"tabId":id,"css":css}))?;
                } else {
                    self.store.custom_css = CustomCss {
                        path: Some(path.clone()),
                        content: content.clone(),
                    };
                    out.result = json!({"success":true,"path":path,"content":content});
                    out.event("css:content", &self.store.custom_css)?;
                }
            }
            "css_history_remove" => {
                let path: String = arg(args, "path")?;
                self.store
                    .custom_css_history
                    .retain(|entry| entry.path != path);
                out.result = self.read(
                    "css_history_get",
                    &json!({"availablePaths":assets.keys().collect::<Vec<_>>()}),
                )?;
            }
            "css_tab_set" | "css_tab_clear" | "css_tab_toggle" => {
                let id: String = arg(args, "tabId")?;
                let css = if command == "css_tab_clear" {
                    None
                } else if command == "css_tab_toggle" {
                    let enabled: bool = arg(args, "enabled")?;
                    let mut css =
                        self.store
                            .tab_css_overrides
                            .get(&id)
                            .cloned()
                            .unwrap_or(TabCss {
                                path: None,
                                content: String::new(),
                                enabled,
                            });
                    css.enabled = enabled;
                    Some(css)
                } else {
                    let mut css: Option<TabCss> = optional(args, "css")?;
                    if let Some(css) = css.as_mut() {
                        if css
                            .path
                            .as_deref()
                            .is_some_and(|path| css_content(path, &assets).is_err())
                        {
                            css.path = None;
                        }
                    }
                    css
                };
                if let Some(css) = &css {
                    self.store.tab_css_overrides.insert(id.clone(), css.clone());
                } else {
                    self.store.tab_css_overrides.remove(&id);
                }
                out.result = json!({"success":true,"tabId":id});
                if command == "css_tab_toggle" {
                    out.result["enabled"] = arg::<Value>(args, "enabled")?;
                } else if command == "css_tab_set" {
                    if let Some(css) = &css {
                        out.result["css"] = value(css)?;
                    }
                }
                out.event("tabCss:changed", json!({"tabId":id,"css":css}))?;
            }
            _ => return Err(format!("UNSUPPORTED_WEB_COMMAND:{command}")),
        }
        Ok(out)
    }
    pub(in crate::web_session) fn css_history_items(&self, assets: &WebAssetMap) -> Value {
        Value::Array(
            self.store
                .custom_css_history
                .iter()
                .map(|entry| {
                    let status = if !assets.contains_key(&entry.path) {
                        "missing"
                    } else {
                        match css_content(&entry.path, assets) {
                            Ok(_) => "available",
                            Err("TOO_LARGE") => "tooLarge",
                            Err(_) => "invalid",
                        }
                    };
                    json!({"path":entry.path,"lastUsedAt":entry.last_used_at,"status":status})
                })
                .collect(),
        )
    }
    pub(in crate::web_session) fn css_export(&self, args: &Value) -> Result<Value, String> {
        let id: String = arg(args, "tabId")?;
        let Some(css) = self
            .store
            .tab_css_overrides
            .get(&id)
            .filter(|css| !css.content.is_empty())
        else {
            return Ok(json!({"success":false}));
        };
        Ok(json!({"success":true,"path":format!("{id}.css"),"content":css.content}))
    }
}
