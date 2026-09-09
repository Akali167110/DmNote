use crate::{errors::EditorCommitError, models::CounterAnimationPreset};
pub fn apply_preset_to_bound_counters(
    store: &mut crate::models::AppStoreData,
    preset_id: &str,
    preset: &CounterAnimationPreset,
) -> u32 {
    let mut affected = 0u32;

    for positions in store.key_positions.values_mut() {
        for position in positions.iter_mut() {
            if update_counter_animation_if_bound(&mut position.counter, preset_id, preset) {
                affected += 1;
            }
        }
    }

    for positions in store.stat_positions.values_mut() {
        for position in positions.iter_mut() {
            if update_counter_animation_if_bound(&mut position.position.counter, preset_id, preset)
            {
                affected += 1;
            }
        }
    }

    for positions in store.graph_positions.values_mut() {
        for position in positions.iter_mut() {
            if update_counter_animation_if_bound(&mut position.position.counter, preset_id, preset)
            {
                affected += 1;
            }
        }
    }

    affected
}

pub fn apply_fallback_to_bound_counters(
    store: &mut crate::models::AppStoreData,
    preset_id: &str,
    fallback: &CounterAnimationPreset,
) -> u32 {
    apply_preset_to_bound_counters(store, preset_id, fallback)
}

pub fn replace_counter_animation_preset(
    store: &mut crate::models::AppStoreData,
    preset_id: &str,
    replacement: &CounterAnimationPreset,
) -> Result<(), EditorCommitError> {
    let Some(item) = store
        .counter_animation_presets
        .iter_mut()
        .find(|item| item.id == preset_id)
    else {
        return Err(EditorCommitError::validation(
            "COUNTER_ANIMATION_PRESET_NOT_FOUND",
            format!("counter animation preset not found: {preset_id}"),
        ));
    };
    item.clone_from(replacement);
    Ok(())
}

pub fn remove_counter_animation_preset(
    store: &mut crate::models::AppStoreData,
    preset_id: &str,
) -> Result<(), EditorCommitError> {
    let before = store.counter_animation_presets.len();
    store
        .counter_animation_presets
        .retain(|preset| preset.id != preset_id);
    if store.counter_animation_presets.len() == before {
        return Err(EditorCommitError::validation(
            "COUNTER_ANIMATION_PRESET_NOT_FOUND",
            format!("counter animation preset not found: {preset_id}"),
        ));
    }
    Ok(())
}

fn update_counter_animation_if_bound(
    counter: &mut crate::models::KeyCounterSettings,
    preset_id: &str,
    preset: &CounterAnimationPreset,
) -> bool {
    let bound_id = counter
        .animation
        .preset_id
        .as_ref()
        .map(|value| value.trim())
        .unwrap_or_default();

    if bound_id != preset_id {
        return false;
    }

    counter.animation.preset_id = Some(preset.id.clone());
    counter.animation.bezier = preset.bezier;
    counter.animation.scale = preset.scale;
    counter.animation.duration_ms = preset.duration_ms;
    counter.normalize();
    true
}
