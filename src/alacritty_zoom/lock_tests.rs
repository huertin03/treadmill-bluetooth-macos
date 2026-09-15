use super::*;
use crate::alacritty_zoom::ZoomOp;
use crate::alacritty_zoom::operations::ZoomCore;
use crate::alacritty_zoom::test_support::Fake;
use std::os::unix::fs::PermissionsExt;
use crate::alacritty_zoom::test_support::TestLockDir;

#[tokio::test(start_paused = true)]
async fn waits_for_guard_drop_and_keeps_private_lock_file() {
    let dir = TestLockDir::create();
    let path = dir.path();
    let first = ZoomLock::acquire(&path).await.unwrap();
    assert!(ZoomLock::try_acquire(&path).unwrap().is_none());
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    let second_path = path.clone();
    let second = tokio::spawn(async move { ZoomLock::acquire(&second_path).await });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(!second.is_finished());
    drop(first);
    let second = second.await.unwrap().unwrap();
    assert!(ZoomLock::try_acquire(&path).unwrap().is_none());
    drop(second);
    assert!(path.exists());
    assert!(ZoomLock::try_acquire(&path).unwrap().is_some());
}

#[tokio::test(start_paused = true)]
async fn lock_timeout_is_an_error_and_does_not_release_other_owner() {
    let dir = TestLockDir::create();
    let path = dir.path();
    let _first = ZoomLock::acquire(&path).await.unwrap();
    let started = tokio::time::Instant::now();
    let error = ZoomLock::acquire(&path).await.err().unwrap();
    assert!(error.to_string().contains("lock timeout"));
    assert_eq!(started.elapsed(), ZOOM_LOCK_WAIT);
    assert!(ZoomLock::try_acquire(&path).unwrap().is_none());
}

#[tokio::test(start_paused = true)]
async fn reset_waits_for_inflight_apply_then_rereads_and_reverts() {
    let dir = TestLockDir::create();
    let fake = Fake::default();
    let id = fake.add(1, 10, 14.0);
    {
        let mut state = fake.0.lock().unwrap();
        state.lock_path = Some(dir.path());
        state.call_delay = Some(Duration::from_millis(100));
    }
    let mut apply_core = ZoomCore::new(fake.clone());
    let apply = tokio::spawn(async move { apply_core.run_op(ZoomOp::Apply { delta_pt: 0.625 }).await });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_millis(100)).await;
    tokio::task::yield_now().await;
    // The recovery row exists, but the set is still sleeping.
    assert_eq!(fake.0.lock().unwrap().records.len(), 1);
    assert_eq!(fake.size(id), 14.0);
    let mut reset_core = ZoomCore::new(fake.clone());
    let reset = tokio::spawn(async move { reset_core.run_op(ZoomOp::Revert).await });
    tokio::task::yield_now().await;
    assert_eq!(fake.0.lock().unwrap().scans, 1);
    assert!(!reset.is_finished());
    apply.await.unwrap().unwrap();
    reset.await.unwrap().unwrap();
    assert_eq!(fake.size(id), 14.0);
    assert!(fake.0.lock().unwrap().records.is_empty());
    let calls = fake.0.lock().unwrap().log.len();
    ZoomCore::new(fake.clone()).run_op(ZoomOp::Revert).await.unwrap();
    assert_eq!(fake.0.lock().unwrap().log.len(), calls);
}
