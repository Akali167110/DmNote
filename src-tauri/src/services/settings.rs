use std::sync::Arc;

use anyhow::{Context, Result};

use crate::models::{SettingsDiff, SettingsPatchInput, SettingsState};
use crate::state::AppStore;

#[derive(Clone)]
pub struct SettingsService {
    store: Arc<AppStore>,
}

impl SettingsService {
    pub fn new(store: Arc<AppStore>) -> Self {
        Self { store }
    }

    pub fn snapshot(&self) -> SettingsState {
        self.store.settings_snapshot()
    }

    pub fn apply_patch(&self, patch: SettingsPatchInput) -> Result<SettingsDiff> {
        let mut diff = None;
        self.store.update(|state| {
            diff = Some(apply_patch_to_store(state, &patch));
        })?;
        diff.context("settings patch did not produce a diff")
    }
}

pub(crate) use dmnote_editor_engine::settings::{apply_patch_to_store, settings_from_store};
