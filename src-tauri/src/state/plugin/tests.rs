use super::*;
#[test]
fn authority_generation_advances_and_rejects_stale_leases() {
    let authority = PluginRuntimeAuthority::default();
    assert_eq!(
        authority.admit(0).unwrap_err(),
        "AUTHORITY_UNAVAILABLE".to_string()
    );
    let generation = authority.reset().unwrap().generation();
    assert_eq!(generation, 1);
    assert!(authority.admit(1).is_ok());
    assert_eq!(
        authority.admit(0).unwrap_err(),
        "AUTHORITY_GENERATION_CHANGED".to_string()
    );
    authority.mark_unavailable();
    assert_eq!(
        authority.admit(1).unwrap_err(),
        "AUTHORITY_UNAVAILABLE".to_string()
    );
}

// lease가 잠금을 들면 번호표 turn 대기 중 reset·다른 admit이 막혀 교착한다
#[test]
fn admission_does_not_hold_authority_lock() {
    let authority = std::sync::Arc::new(PluginRuntimeAuthority::default());
    authority.reset().unwrap();
    let lease = authority.admit(1).unwrap();

    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let worker_authority = std::sync::Arc::clone(&authority);
    let worker = std::thread::spawn(move || {
        let reset = worker_authority.reset().unwrap();
        let other = worker_authority.admit(reset.generation());
        done_tx.send(other.is_ok()).unwrap();
    });

    assert!(done_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("lease 보유 중에도 reset·admit이 진행돼야 한다"));
    worker.join().unwrap();
    assert_eq!(lease.generation(), 1);
}

#[test]
fn revalidate_rejects_after_reset_and_after_unavailable() {
    let authority = PluginRuntimeAuthority::default();
    authority.reset().unwrap();
    let lease = authority.admit(1).unwrap();
    assert!(authority.revalidate(lease).is_ok());

    authority.reset().unwrap();
    assert_eq!(
        authority.revalidate(lease).unwrap_err(),
        "AUTHORITY_GENERATION_CHANGED".to_string()
    );

    let fresh = authority.admit(2).unwrap();
    authority.mark_unavailable();
    assert_eq!(
        authority.revalidate(fresh).unwrap_err(),
        "AUTHORITY_UNAVAILABLE".to_string()
    );
}
