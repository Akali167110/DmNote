use super::*;
fn session() -> WebEditorSession {
    let migrated: Value =
        serde_json::from_str(&crate::session::migrate_store_json("{}", 0).unwrap()).unwrap();
    WebEditorSession::new(&migrated["store"].to_string()).unwrap()
}
fn read(session: &WebEditorSession, command: &str, args: Value) -> Value {
    serde_json::from_str(&session.read(command, &args.to_string()).unwrap()).unwrap()
}
fn run(session: &mut WebEditorSession, command: &str, args: Value) -> Value {
    let token = uuid::Uuid::new_v4().to_string();
    let prepared = session
        .prepare_command(command, &args.to_string(), &token)
        .unwrap();
    let confirmed = session.confirm_command(&token).unwrap();
    assert_eq!(prepared, confirmed);
    serde_json::from_str(&confirmed).unwrap()
}
fn edit_request(session: &WebEditorSession, offset: f64) -> Value {
    let doc = read(session, "editor_get", json!({}));
    let mut positions = doc["document"]["keyPositions"].clone();
    positions["4key"][0]["dx"] = json!(positions["4key"][0]["dx"].as_f64().unwrap() + offset);
    json!({"request":{"baseRevision":doc["revision"],"mutationId":uuid::Uuid::new_v4().to_string(),"changes":{"schemaVersion":1,"keyPositions":positions}}})
}
#[test]
fn persistence_failure_keeps_document_history_and_mutation_ack_unpublished() {
    let mut s = session();
    let before = s.snapshot().unwrap();
    let args = edit_request(&s, 3.0);
    let prepared: Value = serde_json::from_str(
        &s.prepare_command("editor_commit", &args.to_string(), "pending")
            .unwrap(),
    )
    .unwrap();
    assert!(!prepared["store"].is_null());
    assert_eq!(s.snapshot().unwrap(), before);
    assert!(s
        .prepare_command("editor_commit", &args.to_string(), "other")
        .unwrap_err()
        .contains("PENDING"));
    s.discard_command("pending").unwrap();
    assert_eq!(s.snapshot().unwrap(), before);
    let first = run(&mut s, "editor_commit", args.clone());
    let replay = run(&mut s, "editor_commit", args);
    assert_eq!(first["result"], replay["result"]);
    assert!(replay["store"].is_null());
    assert_eq!(replay["events"], json!([]));
}
#[test]
fn history_prepare_is_atomic_and_undo_redo_restore_real_editor_fields() {
    let mut s = session();
    let original = read(&s, "editor_get", json!({}));
    let args = edit_request(&s, 7.0);
    run(&mut s, "editor_commit", args);
    let edited = read(&s, "editor_get", json!({}));
    let before = s.snapshot().unwrap();
    let undo = json!({"operationId":uuid::Uuid::new_v4().to_string()});
    s.prepare_command("history_undo", &undo.to_string(), "undo")
        .unwrap();
    s.discard_command("undo").unwrap();
    assert_eq!(s.snapshot().unwrap(), before);
    run(&mut s, "history_undo", undo.clone());
    assert_eq!(
        read(&s, "editor_get", json!({}))["document"],
        original["document"]
    );
    let replay = run(&mut s, "history_undo", undo);
    assert!(replay["store"].is_null());
    run(
        &mut s,
        "history_redo",
        json!({"operationId":uuid::Uuid::new_v4().to_string()}),
    );
    assert_eq!(
        read(&s, "editor_get", json!({}))["document"],
        edited["document"]
    );
}
#[test]
fn plugin_instance_namespace_and_authority_use_native_history_rules() {
    let mut s = session();
    let authority =
        run(&mut s, "plugin_authority_reset", json!({}))["result"]["authorityGeneration"].clone();
    let instance = json!({"instanceId":uuid::Uuid::new_v4().to_string(),"position":{"x":12,"y":7},"tabId":"4key","hidden":false});
    let commit = json!({"request":{"pluginId":"demo","instances":[instance],"mutationId":uuid::Uuid::new_v4().to_string(),"authorityGeneration":authority,"expectedModelRevision":0}});
    run(&mut s, "plugin_instances_commit", commit);
    assert_eq!(
        read(&s, "plugin_instances_get", json!({"pluginId":"demo"}))["instances"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(s
        .prepare_command(
            "plugin_storage_set",
            &json!({"key":"demo/instances","value":[]}).to_string(),
            "bad"
        )
        .unwrap_err()
        .contains("RESERVED"));
    run(
        &mut s,
        "history_undo",
        json!({"operationId":uuid::Uuid::new_v4().to_string()}),
    );
    assert_eq!(
        read(&s, "plugin_instances_get", json!({"pluginId":"demo"}))["instances"],
        json!([])
    );
    run(
        &mut s,
        "history_redo",
        json!({"operationId":uuid::Uuid::new_v4().to_string()}),
    );
    run(
        &mut s,
        "plugin_storage_set",
        json!({"key":"demo/settings","value":{"theme":"dark"}}),
    );
    run(
        &mut s,
        "plugin_storage_clear_by_prefix",
        json!({"prefix":"demo/"}),
    );
    assert_eq!(
        read(&s, "plugin_storage_has_data", json!({"prefix":"demo/"})),
        json!(false)
    );
    assert_eq!(
        read(&s, "history_status", json!({}))["canUndo"],
        json!(false)
    );
}
#[test]
fn web_keyboard_binding_retains_all_any_and_physical_repeat_rules() {
    let keys = json!({"4key":[{"keys":["A","B"],"match":"all"}]});
    let mut matcher = crate::keyboard::WebKeyboardMatcher::new(&keys.to_string(), "4key").unwrap();
    let feed =
        |matcher: &mut crate::keyboard::WebKeyboardMatcher, key: &str, down: bool| -> Value {
            serde_json::from_str(
            &matcher
                .feed(
                    &json!({"physicalId":key,"device":"keyboard","candidates":[key],"isDown":down})
                        .to_string(),
                )
                .unwrap(),
        )
        .unwrap()
        };
    let first = feed(&mut matcher, "A", true);
    assert_eq!(first["events"][0]["transition"], Value::Null);
    assert_eq!(feed(&mut matcher, "A", true), Value::Null);
    assert_eq!(
        feed(&mut matcher, "B", true)["events"][0]["transition"],
        json!(true)
    );
    assert_eq!(
        feed(&mut matcher, "A", false)["events"][0]["transition"],
        json!(false)
    );
}

#[test]
fn persisted_checkpoint_restores_receipt_authority_without_restoring_undo_history() {
    let mut first = session();
    let lease = run(&mut first, "plugin_authority_reset", json!({}));
    let edit = edit_request(&first, 4.0);
    run(&mut first, "editor_commit", edit);
    let persisted: Value = serde_json::from_str(&first.snapshot().unwrap()).unwrap();
    let mut restarted = WebEditorSession::new(&persisted["store"].to_string()).unwrap();
    restarted
        .restore_checkpoint(&persisted["checkpoint"].to_string())
        .unwrap();
    assert!(
        read(&restarted, "history_status", json!({}))["statusSeq"]
            .as_u64()
            .unwrap()
            > persisted["history"]["statusSeq"].as_u64().unwrap()
    );
    assert_eq!(
        read(&restarted, "history_status", json!({}))["canUndo"],
        json!(false)
    );
    let request = json!({"request":{"pluginId":"resumed","instances":[],"mutationId":uuid::Uuid::new_v4().to_string(),"authorityGeneration":lease["result"]["authorityGeneration"]}});
    run(&mut restarted, "plugin_instances_commit", request);
    assert!(restarted
        .restore_checkpoint(&persisted["checkpoint"].to_string())
        .unwrap_err()
        .contains("ALREADY_INITIALIZED"));
}

#[test]
fn custom_tab_crud_and_reset_share_native_history_and_list_contracts() {
    let mut session = session();
    let created = run(
        &mut session,
        "custom_tabs_create",
        json!({"name":"Browser"}),
    );
    let id = created["result"]["result"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        read(&session, "custom_tabs_list", json!({}))[0]["id"],
        json!(id)
    );
    assert_eq!(
        read(&session, "history_status", json!({}))["canUndo"],
        json!(true)
    );
    run(
        &mut session,
        "history_undo",
        json!({"operationId":uuid::Uuid::new_v4().to_string()}),
    );
    assert_eq!(read(&session, "custom_tabs_list", json!({})), json!([]));
    run(
        &mut session,
        "history_redo",
        json!({"operationId":uuid::Uuid::new_v4().to_string()}),
    );
    run(&mut session, "keys_reset_all", json!({}));
    assert_eq!(read(&session, "custom_tabs_list", json!({})), json!([]));
    assert_eq!(
        read(&session, "history_status", json!({}))["canUndo"],
        json!(false)
    );
}

#[test]
fn web_key_presses_update_enabled_counters_without_changing_editor_history() {
    let mut session = session();
    let mode = "4key";
    let key = session.state.store.keys[mode][0].canonical();
    run(
        &mut session,
        "settings_update",
        json!({"patch":{"keyCounterEnabled":true}}),
    );
    let history = read(&session, "history_status", json!({}));
    let result = run(
        &mut session,
        "web_input_press",
        json!({"mode":mode,"keys":[key]}),
    );
    assert_eq!(result["events"][0]["event"], json!("keys:counter"));
    assert_eq!(result["events"][0]["payload"]["count"], json!(1));
    assert_eq!(read(&session, "history_status", json!({})), history);
}

#[test]
fn compound_gesture_saves_editor_and_plugin_together_and_undo_restores_both() {
    let mut session = session();
    let authority = run(&mut session, "plugin_authority_reset", json!({}))["result"]
        ["authorityGeneration"]
        .clone();
    let original = read(&session, "editor_get", json!({}));
    let edit = edit_request(&session, 9.0);
    let request = json!({"request":{"gestureId":uuid::Uuid::new_v4().to_string(),"mutationId":uuid::Uuid::new_v4().to_string(),"editorBaseRevision":original["revision"],"pluginBaseRevision":0,"authorityGeneration":authority,"editorChanges":edit["request"]["changes"],"pluginChanges":[{"pluginId":"compound","instances":[{"instanceId":uuid::Uuid::new_v4().to_string(),"position":{"x":3,"y":8},"tabId":"4key","hidden":false}]}]}});
    let before = session.snapshot().unwrap();
    session
        .prepare_command("commit_gesture", &request.to_string(), "joint")
        .unwrap();
    assert_eq!(session.snapshot().unwrap(), before);
    session.discard_command("joint").unwrap();
    run(&mut session, "commit_gesture", request.clone());
    let replay = run(&mut session, "commit_gesture", request);
    assert!(replay["store"].is_null());
    assert_eq!(replay["events"], json!([]));
    run(
        &mut session,
        "history_undo",
        json!({"operationId":uuid::Uuid::new_v4().to_string()}),
    );
    assert_eq!(
        read(&session, "editor_get", json!({}))["document"],
        original["document"]
    );
    assert_eq!(
        read(
            &session,
            "plugin_instances_get",
            json!({"pluginId":"compound"})
        )["instances"],
        json!([])
    );
}

#[test]
fn main_lifecycle_invalidates_only_authority_after_durable_confirmation() {
    let mut session = session();
    let lease = run(&mut session, "plugin_authority_reset", json!({}))["result"]
        ["authorityGeneration"]
        .clone();
    let before = session.snapshot().unwrap();
    let prepared: Value = serde_json::from_str(
        &session
            .prepare_command("web_main_disconnected", "{}", "disconnect")
            .unwrap(),
    )
    .unwrap();
    assert!(prepared["store"].is_null());
    assert_eq!(prepared["checkpoint"]["authorityAvailable"], json!(false));
    assert_eq!(session.snapshot().unwrap(), before);
    session.discard_command("disconnect").unwrap();
    assert_eq!(session.snapshot().unwrap(), before);
    run(&mut session, "web_main_disconnected", json!({}));
    let request = json!({"request":{"pluginId":"demo","instances":[],"mutationId":uuid::Uuid::new_v4().to_string(),"authorityGeneration":lease}});
    assert_eq!(
        session
            .prepare_command("plugin_instances_commit", &request.to_string(), "stale")
            .unwrap_err(),
        "AUTHORITY_UNAVAILABLE"
    );
    let fresh = run(&mut session, "plugin_authority_reset", json!({}));
    assert_eq!(
        fresh["result"]["authorityGeneration"].as_u64().unwrap(),
        lease.as_u64().unwrap() + 1
    );
    assert_eq!(
        session
            .prepare_command("plugin_instances_commit", &request.to_string(), "stale")
            .unwrap_err(),
        "AUTHORITY_GENERATION_CHANGED"
    );
}
