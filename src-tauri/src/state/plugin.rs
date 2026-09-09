use super::editor::MAX_SAFE_WIRE_REVISION;
pub use dmnote_editor_engine::state::plugin::*;
use parking_lot::Mutex;
#[derive(Debug, Default)]
struct PluginAuthorityState {
    generation: u64,
    available: bool,
}

#[derive(Debug, Default)]
pub(crate) struct PluginRuntimeAuthority {
    state: Mutex<PluginAuthorityState>,
}

// 입장 허가 - generation 스냅샷만 든다. 잠금을 든 채 번호표 turn을 기다리면
// 앞 번호표가 admit에서 이 잠금을 기다려 교착한다 (잠금 순서: 번호표 turn → authority).
// reset과의 배제는 번호표 FIFO가 맡고, 커맨드는 turn 안에서 revalidate로 다시 확인한다
#[derive(Debug, Clone, Copy)]
pub(crate) struct PluginAuthorityLease {
    generation: u64,
}

impl PluginAuthorityLease {
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
}

impl PluginRuntimeAuthority {
    pub(crate) fn admit(&self, expected_generation: u64) -> Result<PluginAuthorityLease, String> {
        let guard = self.state.lock();
        if !guard.available {
            return Err("AUTHORITY_UNAVAILABLE".to_string());
        }
        if guard.generation != expected_generation {
            return Err("AUTHORITY_GENERATION_CHANGED".to_string());
        }
        Ok(PluginAuthorityLease {
            generation: guard.generation,
        })
    }

    // 번호표 turn 안 재확인 - turn 대기 중 reset이 끼어든 커밋을 거절
    pub(crate) fn revalidate(&self, lease: PluginAuthorityLease) -> Result<(), String> {
        self.admit(lease.generation()).map(drop)
    }

    pub(crate) fn reset(&self) -> Result<PluginAuthorityLease, String> {
        let mut state = self.state.lock();
        state.generation = state
            .generation
            .checked_add(1)
            .filter(|generation| *generation <= MAX_SAFE_WIRE_REVISION)
            .ok_or_else(|| "AUTHORITY_GENERATION_OUT_OF_RANGE".to_string())?;
        state.available = true;
        Ok(PluginAuthorityLease {
            generation: state.generation,
        })
    }

    pub(crate) fn generation(&self) -> u64 {
        self.state.lock().generation
    }

    pub(crate) fn mark_unavailable(&self) {
        let mut state = self.state.lock();
        state.available = false;
    }
}

#[cfg(test)]
mod tests;
