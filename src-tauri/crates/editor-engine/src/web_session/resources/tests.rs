use super::*;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
fn session() -> WebEditorSession {
    WebEditorSession::new(&encode(&AppStoreData::default()).unwrap()).unwrap()
}
fn apply(session: &mut WebEditorSession, command: &str, args: Value) -> Value {
    let response: Value = serde_json::from_str(
        &session
            .prepare_command(command, &encode(&args).unwrap(), "test-resource")
            .unwrap(),
    )
    .unwrap();
    session.confirm_command("test-resource").unwrap();
    response
}
#[test]
fn css_load_history_and_tab_activation_share_assets_and_preserve_enable_state() {
    let mut session = session();
    let original: Value = serde_json::from_str(&session.read("css_get", "{}").unwrap()).unwrap();
    let prepared:Value=serde_json::from_str(&session.prepare_command("css_load",&encode(&json!({"files":[{"name":"style.css","dataBase64":BASE64_STANDARD.encode(b"body {color:red}")}],"timestampMs":123})).unwrap(),"css").unwrap()).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&session.read("css_get", "{}").unwrap()).unwrap(),
        original
    );
    session.confirm_command("css").unwrap();
    assert_eq!(session.read("css_get_use", "{}").unwrap(), "false");
    let path = prepared["result"]["path"].as_str().unwrap();
    let assets = prepared["assetWrites"].clone();
    let activated = apply(
        &mut session,
        "css_tab_activate_history",
        json!({"tabId":"4key","path":path,"assets":assets,"timestampMs":456}),
    );
    assert_eq!(activated["result"]["css"]["enabled"], true);
    assert_eq!(activated["result"]["css"]["content"], "body {color:red}");
    let rejected = apply(
        &mut session,
        "css_history_activate",
        json!({"path":"/assets/css/unknown.css","assets":assets,"timestampMs":789}),
    );
    assert_eq!(rejected["result"]["code"], "PATH_NOT_AUTHORIZED");
}
#[test]
fn js_file_reload_reads_persisted_source_and_forces_same_content_publication() {
    let mut session = session();
    let loaded = apply(
        &mut session,
        "js_load",
        json!({"files":[{"name":"example.js","dataBase64":BASE64_STANDARD.encode(b"console.log('source')")}]}),
    );
    let id = loaded["result"]["added"][0]["id"].as_str().unwrap();
    apply(&mut session, "js_set_content", json!({"content":"edited"}));
    let reloaded = apply(
        &mut session,
        "js_reload",
        json!({"assets":loaded["assetWrites"]}),
    );
    assert_eq!(
        reloaded["result"]["updated"][0]["content"],
        "console.log('source')"
    );
    assert!(reloaded["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["event"] == "js:content" && event["payload"]["forced"] == true));
    let removed = apply(&mut session, "js_remove_plugin", json!({"id":id}));
    assert_eq!(removed["result"]["removed_id"], id);
}
#[test]
fn preset_wire_validation_rejects_invalid_rotation_before_any_mutation() {
    let mut session = session();
    let before = session.snapshot().unwrap();
    let preset = json!({"keys":{"4key":["A"]},"keyPositions":{"4key":[{"rotation":9999}]}});
    assert!(session
        .prepare_command(
            "preset_load",
            &encode(&json!({"preset":preset})).unwrap(),
            "bad-preset"
        )
        .is_err());
    assert_eq!(session.snapshot().unwrap(), before);
}
#[test]
fn settings_and_tab_note_values_only_publish_after_confirmation() {
    let mut session = session();
    let before = session.read("settings_get", "{}").unwrap();
    session
        .prepare_command(
            "settings_update",
            &encode(&json!({"patch":{"backgroundColor":"#123456"}})).unwrap(),
            "settings",
        )
        .unwrap();
    assert_eq!(session.read("settings_get", "{}").unwrap(), before);
    session.discard_command("settings").unwrap();
    assert_eq!(session.read("settings_get", "{}").unwrap(), before);
    let result = apply(&mut session, "note_tab_clear", json!({"tabId":"4key"}));
    assert!(
        result["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["event"] == "tabNote:changed"
                && event["payload"]["settings"].is_null())
    );
}
