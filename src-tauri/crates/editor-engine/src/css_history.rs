use crate::{local_asset_path::path_identity_key, models::CustomCssHistoryEntry};
use std::{collections::HashSet, path::Path};
pub const MAX_CUSTOM_CSS_HISTORY_ENTRIES: usize = 10;
pub fn normalize_custom_css_history(history: &mut Vec<CustomCssHistoryEntry>) -> bool {
    let original = history.clone();
    history.retain(|entry| Path::new(&entry.path).is_absolute());
    history.sort_by_key(|entry| std::cmp::Reverse(entry.loaded_at));

    let mut seen = HashSet::with_capacity(history.len());
    history.retain(|entry| seen.insert(path_identity_key(Path::new(&entry.path))));
    while history.len() > MAX_CUSTOM_CSS_HISTORY_ENTRIES {
        let eviction_index = history
            .iter()
            .enumerate()
            .min_by_key(|(_, entry)| (entry.last_used_at, entry.loaded_at))
            .map(|(index, _)| index)
            .expect("CSS history must contain an eviction candidate");
        history.remove(eviction_index);
    }
    history.sort_by_key(|entry| std::cmp::Reverse(entry.loaded_at));

    *history != original
}

pub fn migrate_custom_css_history_timestamps(history: &mut [CustomCssHistoryEntry]) -> bool {
    let mut changed = false;
    for entry in history.iter_mut().filter(|entry| entry.loaded_at == 0) {
        entry.loaded_at = entry.last_used_at;
        changed = true;
    }
    changed
}

pub fn migrate_custom_css_history_at_load(
    history: &mut Vec<CustomCssHistoryEntry>,
    active_path: Option<&str>,
    timestamp: i64,
) -> bool {
    let original = history.clone();
    migrate_custom_css_history_timestamps(history);

    if let Some(path) = active_path.filter(|path| {
        Path::new(path).is_absolute()
            && !history
                .iter()
                .any(|entry| history_paths_match(&entry.path, path))
    }) {
        history.push(CustomCssHistoryEntry {
            path: path.to_string(),
            loaded_at: timestamp,
            last_used_at: timestamp,
        });
    }

    normalize_custom_css_history(history);
    *history != original
}

fn history_paths_match(left: &str, right: &str) -> bool {
    path_identity_key(Path::new(left)) == path_identity_key(Path::new(right))
}
