use crate::{ipc::InputDeviceKind, models::KeyMappings};
use dmnote_editor_engine::keyboard::KeyboardMatcher;
pub use dmnote_editor_engine::keyboard::{collect_sound_dispatch, MatchOutcome, SlotEvent};
use parking_lot::RwLock;
use std::sync::Arc;
#[derive(Clone)]
pub struct KeyboardManager {
    state: Arc<RwLock<KeyboardMatcher>>,
}
impl KeyboardManager {
    pub fn new(initial: KeyMappings, mode: impl Into<String>) -> Self {
        Self {
            state: Arc::new(RwLock::new(KeyboardMatcher::new(initial, mode))),
        }
    }
    pub fn set_mode(&self, mode: impl Into<String>) -> bool {
        self.state.write().set_mode(mode)
    }
    pub fn update_mappings_and_set_mode(
        &self,
        mappings: KeyMappings,
        mode: impl Into<String>,
    ) -> bool {
        self.state
            .write()
            .update_mappings_and_set_mode(mappings, mode)
    }
    pub fn update_mappings(&self, mappings: KeyMappings) {
        self.state.write().update_mappings(mappings)
    }
    pub fn current_mode(&self) -> String {
        self.state.read().current_mode()
    }
    pub fn match_and_register<'a>(
        &self,
        physical_id: Option<&str>,
        device: InputDeviceKind,
        candidates: impl IntoIterator<Item = &'a str>,
        is_down: bool,
    ) -> Option<MatchOutcome> {
        self.state
            .write()
            .match_and_register(physical_id, device, candidates, is_down)
    }
    pub fn clear_active_keys(&self) {
        self.state.write().clear_active_keys()
    }
    pub fn current_mode_and_pressed_keys(&self) -> (String, Vec<String>) {
        self.state.read().current_mode_and_pressed_keys()
    }
    #[cfg(test)]
    pub fn pressed_keys(&self) -> Vec<String> {
        self.current_mode_and_pressed_keys().1
    }
    #[cfg(test)]
    pub fn register_key_down(&self, mode: &str, key: &str) -> bool {
        if self.current_mode() != mode {
            return false;
        }
        self.match_and_register(
            Some(&format!("test:key:{key}")),
            InputDeviceKind::Keyboard,
            [key],
            true,
        )
        .is_some_and(|outcome| {
            outcome.pressed_label.is_some()
                && outcome.events.iter().any(|e| e.transition == Some(true))
        })
    }
    #[cfg(test)]
    pub fn register_key_up(&self, mode: &str, key: &str) -> bool {
        if self.current_mode() != mode {
            return false;
        }
        self.match_and_register(
            Some(&format!("test:key:{key}")),
            InputDeviceKind::Keyboard,
            [key],
            false,
        )
        .is_some_and(|outcome| outcome.events.iter().any(|e| e.transition == Some(false)))
    }
}
#[cfg(test)]
mod tests;
