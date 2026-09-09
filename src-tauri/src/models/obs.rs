pub use dmnote_editor_engine::models::obs::*;
pub fn make_envelope(msg_type: &str, seq: u64, payload: serde_json::Value) -> serde_json::Value {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    make_envelope_at(msg_type, seq, payload, timestamp)
}
