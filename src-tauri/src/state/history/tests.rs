use super::*;
use crate::keyboard::KeyboardManager;
use std::{
    collections::HashMap,
    sync::{mpsc, Arc},
    thread,
    time::{Duration, Instant},
};

#[test]
fn admission_gate_rejects_busy_and_stale_generations() {
    let gate = Arc::new(HistoryAdmissionGate::default());
    let stale = gate.try_admit().unwrap();
    let lease = gate.close("operation-a").unwrap();
    assert_eq!(gate.owner().as_deref(), Some("operation-a"));
    assert_eq!(gate.try_admit().unwrap_err(), HISTORY_IN_PROGRESS);
    drop(lease);

    assert_eq!(gate.revalidate(stale).unwrap_err(), HISTORY_IN_PROGRESS);
    assert!(gate.try_admit().is_ok());
}

#[test]
fn canceled_barrier_wakes_a_drain_waiter() {
    let gate = Arc::new(HistoryAdmissionGate::default());
    let admission = gate.admit_mutation().unwrap();
    let barrier = gate.begin_close("operation-canceled").unwrap();
    let waiter = barrier.waiter();
    let (result_tx, result_rx) = mpsc::channel();
    let wait_thread = thread::spawn(move || {
        result_tx.send(waiter.wait_for_drain()).unwrap();
    });

    drop(barrier);
    assert_eq!(
        result_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        Err(HISTORY_IN_PROGRESS.to_string())
    );
    drop(admission);
    wait_thread.join().unwrap();
}

#[test]
fn barrier_drains_admitted_runtime_publication_before_restore_mapping() {
    let gate = Arc::new(HistoryAdmissionGate::default());
    let keyboard = KeyboardManager::new(
        HashMap::from([("mode".to_string(), vec!["initial".into()])]),
        "mode",
    );
    let admission = gate.admit_mutation().unwrap();
    admission.revalidate_for(&gate).unwrap();
    let stale_keyboard = keyboard.clone();
    let (store_committed_tx, store_committed_rx) = mpsc::channel();
    let (publish_tx, publish_rx) = mpsc::channel();
    let mutation = thread::spawn(move || {
        store_committed_tx.send(()).unwrap();
        publish_rx.recv().unwrap();
        stale_keyboard.update_mappings_and_set_mode(
            HashMap::from([("mode".to_string(), vec!["stale".into()])]),
            "mode",
        );
        drop(admission);
    });
    store_committed_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();

    let barrier_gate = Arc::clone(&gate);
    let restored_keyboard = keyboard.clone();
    let (restored_tx, restored_rx) = mpsc::channel();
    let barrier = thread::spawn(move || {
        let lease = barrier_gate.close("history-operation").unwrap();
        restored_keyboard.update_mappings_and_set_mode(
            HashMap::from([("mode".to_string(), vec!["restored".into()])]),
            "mode",
        );
        restored_tx.send(()).unwrap();
        drop(lease);
    });

    let deadline = Instant::now() + Duration::from_secs(2);
    while !gate.is_closed() {
        assert!(Instant::now() < deadline, "history gate did not close");
        thread::yield_now();
    }
    assert!(matches!(
        restored_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    publish_tx.send(()).unwrap();
    mutation.join().unwrap();
    restored_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    barrier.join().unwrap();

    assert!(keyboard.register_key_down("mode", "restored"));
    assert!(!keyboard.register_key_down("mode", "stale"));
}
