pub use dmnote_editor_engine::state::history::*;
mod admission;
pub(crate) use admission::{
    HistoryAdmission, HistoryAdmissionGate, HistoryAdmissionLease, HistoryBarrierLease,
    HistoryBarrierWaiter,
};
#[cfg(test)]
mod tests;
