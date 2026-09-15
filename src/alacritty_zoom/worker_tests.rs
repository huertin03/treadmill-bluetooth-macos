use super::*;
use crate::alacritty_zoom::IPC_MAX_ATTEMPTS;
use crate::alacritty_zoom::test_support::Fake;

fn enabled() -> ZoomConfig {
    ZoomConfig {
        enabled: true,
        delta_pt: 0.625,
    }
}
async fn settle() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(start_paused = true)]
async fn startup_reverts_and_base_never_scans_then_flapping_coalesces() {
    let fake = Fake::default();
    let id = fake.add(1, 10, 14.625);
    fake.record(id, 14.0, 14.625);
    let (handle, task) = spawn_worker(fake.clone(), enabled());
    settle().await;
    assert_eq!(fake.size(id), 14.0);
    let scans = fake.0.lock().unwrap().scans;
    tokio::time::advance(std::time::Duration::from_secs(20)).await;
    settle().await;
    assert_eq!(fake.0.lock().unwrap().scans, scans);
    handle.set_active(true);
    handle.set_active(false);
    handle.set_active(true);
    settle().await;
    assert_eq!(fake.size(id), 14.625);
    assert_eq!(fake.0.lock().unwrap().scans, scans + 1);
    handle.set_active(false);
    settle().await;
    assert_eq!(fake.size(id), 14.0);
    drop(handle);
    task.await.unwrap();
}
#[tokio::test(start_paused = true)]
async fn late_process_and_restart_are_zoomed_on_one_rescan() {
    let fake = Fake::default();
    let (handle, task) = spawn_worker(fake.clone(), enabled());
    settle().await;
    handle.set_active(true);
    settle().await;
    let old = fake.add(1, 10, 14.0);
    tokio::time::advance(INSTANCE_RESCAN_INTERVAL).await;
    settle().await;
    assert_eq!(fake.size(old), 14.625);
    let calls = fake.0.lock().unwrap().log.len();
    tokio::time::advance(INSTANCE_RESCAN_INTERVAL).await;
    settle().await;
    assert_eq!(fake.0.lock().unwrap().log.len(), calls);
    fake.remove(old);
    let new = fake.add(1, 20, 12.0);
    tokio::time::advance(INSTANCE_RESCAN_INTERVAL).await;
    settle().await;
    assert_eq!(fake.size(new), 12.625);
    handle.set_config(ZoomConfig {
        delta_pt: 1.0,
        ..enabled()
    });
    settle().await;
    assert_eq!(fake.size(new), 13.0);
    handle.set_config(ZoomConfig::default());
    settle().await;
    assert_eq!(fake.size(new), 12.0);
    drop(handle);
    task.await.unwrap();
}
#[tokio::test(start_paused = true)]
async fn timeout_exhaustion_does_not_stop_worker_or_repeat_on_rescan() {
    let fake = Fake::default();
    let id = fake.add(1, 10, 14.0);
    let (handle, task) = spawn_worker(fake.clone(), enabled());
    settle().await;
    fake.0.lock().unwrap().timeout_calls = IPC_MAX_ATTEMPTS;
    handle.set_active(true);
    settle().await;
    for delay in crate::alacritty_zoom::IPC_RETRY_BACKOFF {
        tokio::time::advance(std::time::Duration::from_millis(delay)).await;
        settle().await;
    }
    let calls = fake.0.lock().unwrap().log.len();
    assert_eq!(calls, IPC_MAX_ATTEMPTS);
    tokio::time::advance(INSTANCE_RESCAN_INTERVAL).await;
    settle().await;
    assert_eq!(fake.0.lock().unwrap().log.len(), calls);
    handle.set_config(ZoomConfig {
        delta_pt: 1.0,
        ..enabled()
    });
    settle().await;
    assert_eq!(fake.size(id), 15.0);
    drop(handle);
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn changes_during_ipc_converge_to_latest_state_after_the_op() {
    let fake = Fake::default();
    let id = fake.add(1, 10, 14.0);
    let (handle, task) = spawn_worker(fake.clone(), enabled());
    settle().await;
    fake.0.lock().unwrap().call_delay = Some(std::time::Duration::from_millis(50));
    handle.set_active(true);
    settle().await;
    handle.set_active(false);
    handle.set_active(true);
    handle.set_active(false);
    // Finish the in-flight apply and its immediately following revert.
    for _ in 0..6 {
        tokio::time::advance(std::time::Duration::from_millis(50)).await;
        settle().await;
    }
    assert_eq!(fake.size(id), 14.0);
    assert!(fake.0.lock().unwrap().records.is_empty());
    assert_eq!(fake.0.lock().unwrap().scans, 3);
    drop(handle);
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn failed_revert_retries_without_warn_spam_and_recovers() {
    let (logs, _guard) = crate::alacritty_zoom::test_support::capture_logs();
    let fake = Fake::default();
    let id = fake.add(1, 10, 14.625);
    fake.record(id, 14.0, 14.625);
    fake.0.lock().unwrap().fail_discovery = true;
    let (handle, task) = spawn_worker(fake.clone(), enabled());
    settle().await;
    assert_eq!(fake.0.lock().unwrap().scans, 1);
    for _ in 0..5 {
        tokio::time::advance(INSTANCE_RESCAN_INTERVAL).await;
        settle().await;
    }
    assert_eq!(fake.0.lock().unwrap().scans, 6);
    assert_eq!(logs.count("WARN"), 1);
    assert_eq!(fake.size(id), 14.625);
    fake.0.lock().unwrap().fail_discovery = false;
    tokio::time::advance(INSTANCE_RESCAN_INTERVAL).await;
    settle().await;
    assert_eq!(fake.size(id), 14.0);
    assert_eq!(logs.count("reconciliation recovered"), 1);
    drop(handle);
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn newer_want_wins_during_failure_streak() {
    let fake = Fake::default();
    let id = fake.add(1, 10, 14.625);
    fake.record(id, 14.0, 14.625);
    fake.0.lock().unwrap().fail_discovery = true;
    let (handle, task) = spawn_worker(fake.clone(), enabled());
    settle().await;
    handle.set_active(true);
    handle.set_config(ZoomConfig { delta_pt: 1.0, ..enabled() });
    settle().await;
    fake.0.lock().unwrap().fail_discovery = false;
    tokio::time::advance(INSTANCE_RESCAN_INTERVAL).await;
    settle().await;
    assert_eq!(fake.size(id), 15.0);
    drop(handle);
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn persistent_rescan_error_warns_once() {
    let (logs, _guard) = crate::alacritty_zoom::test_support::capture_logs();
    let fake = Fake::default();
    let (handle, task) = spawn_worker(fake.clone(), enabled());
    settle().await;
    handle.set_active(true);
    settle().await;
    fake.0.lock().unwrap().fail_discovery = true;
    for _ in 0..5 {
        tokio::time::advance(INSTANCE_RESCAN_INTERVAL).await;
        settle().await;
    }
    assert_eq!(logs.count("WARN"), 1);
    fake.0.lock().unwrap().fail_discovery = false;
    tokio::time::advance(INSTANCE_RESCAN_INTERVAL).await;
    settle().await;
    assert_eq!(logs.count("rescan recovered"), 1);
    drop(handle);
    task.await.unwrap();
}
