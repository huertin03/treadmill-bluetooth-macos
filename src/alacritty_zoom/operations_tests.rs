use super::*;
use crate::alacritty_zoom::IPC_MAX_ATTEMPTS;
use crate::alacritty_zoom::test_support::{Fake, capture_logs};
const APPLY: ZoomOp = ZoomOp::Apply { delta_pt: 0.625 };

#[tokio::test(start_paused = true)]
async fn persists_before_setting_and_verifies_both_directions() {
    let fake = Fake::default();
    let id = fake.add(1, 10, 14.0);
    let mut core = ZoomCore::new(fake.clone());
    core.run_op(APPLY).await.unwrap();
    assert_eq!(fake.size(id), 14.625);
    core.run_op(ZoomOp::Revert).await.unwrap();
    assert_eq!(fake.size(id), 14.0);
    let state = fake.0.lock().unwrap();
    assert!(state.records.is_empty());
    assert_eq!(
        state.log,
        [
            "1:get-config -w -1",
            "1:record",
            "1:config -w -1 font.size=14.625",
            "1:get-config -w -1",
            "1:get-config -w -1",
            "1:config -w -1 font.size=14",
            "1:get-config -w -1",
            "1:delete"
        ]
    );
}
#[tokio::test(start_paused = true)]
async fn startup_reverts_only_recorded_instances_and_prunes_dead_records() {
    let fake = Fake::default();
    let id = fake.add(1, 10, 14.625);
    fake.add(2, 20, 12.0);
    fake.record(id, 14.0, 14.625);
    fake.record(
        Identity {
            pid: 3,
            started_at_us: 30,
        },
        10.0,
        11.0,
    );
    let mut core = ZoomCore::new(fake.clone());
    core.run_op(ZoomOp::Revert).await.unwrap();
    let state = fake.0.lock().unwrap();
    assert!(state.records.is_empty());
    assert!(
        !state
            .log
            .iter()
            .any(|line| line.starts_with("2:") || line.starts_with("3:config"))
    );
    assert_eq!(state.sizes[&id], 14.0);
}
#[tokio::test(start_paused = true)]
async fn retries_silent_loss_nonzero_empty_reads_and_timeout() {
    let fake = Fake::default();
    let id = fake.add(1, 10, 14.0);
    {
        let mut state = fake.0.lock().unwrap();
        state.empty_reads = 2;
        state.timeout_calls = 1;
        state.drop_sets = 2;
        state.fail_sets = 1;
    }
    let mut core = ZoomCore::new(fake.clone());
    core.run_op(APPLY).await.unwrap();
    assert_eq!(fake.size(id), 14.625);
    assert!(core.failed.is_empty());
}
#[tokio::test(start_paused = true)]
async fn exhausted_pid_is_suppressed_until_next_op_and_others_continue() {
    let (logs, _guard) = capture_logs();
    let fake = Fake::default();
    let id = fake.add(1, 10, 14.0);
    let second = fake.add(2, 20, 12.0);
    fake.0.lock().unwrap().drop_sets = IPC_MAX_ATTEMPTS;
    let mut core = ZoomCore::new(fake.clone());
    core.run_op(APPLY).await.unwrap();
    assert!(core.failed.contains(&id));
    assert_eq!(logs.count("WARN"), 1);
    assert_eq!(fake.size(second), 12.625);
    let calls = fake.0.lock().unwrap().log.len();
    core.rescan(0.625).await.unwrap();
    assert_eq!(fake.0.lock().unwrap().log.len(), calls);
    assert_eq!(logs.count("WARN"), 1);
    core.run_op(APPLY).await.unwrap();
    assert!(core.failed.is_empty());
    assert_eq!(fake.size(id), 14.625);
    assert_eq!(fake.size(second), 12.625);
}
#[tokio::test(start_paused = true)]
async fn new_delta_and_crash_recovery_keep_the_original_base() {
    let fake = Fake::default();
    let id = fake.add(1, 10, 14.0);
    ZoomCore::new(fake.clone()).run_op(APPLY).await.unwrap();
    let mut restarted = ZoomCore::new(fake.clone());
    restarted
        .run_op(ZoomOp::Apply { delta_pt: 1.0 })
        .await
        .unwrap();
    assert_eq!(fake.size(id), 15.0);
    assert_eq!(fake.0.lock().unwrap().records[0].base_pt, 14.0);
    restarted.run_op(ZoomOp::Revert).await.unwrap();
    assert_eq!(fake.size(id), 14.0);
}
#[tokio::test(start_paused = true)]
async fn external_change_prevents_revert_and_clears_record() {
    let fake = Fake::default();
    let id = fake.add(1, 10, 16.0);
    fake.record(id, 14.0, 14.625);
    ZoomCore::new(fake.clone())
        .run_op(ZoomOp::Revert)
        .await
        .unwrap();
    assert_eq!(fake.size(id), 16.0);
    assert!(fake.0.lock().unwrap().records.is_empty());
}
#[tokio::test(start_paused = true)]
async fn restart_and_pid_reuse_apply_new_identity_and_revert_only_it() {
    for new_pid in [1, 2] {
        let fake = Fake::default();
        let old = fake.add(1, 10, 14.0);
        let mut core = ZoomCore::new(fake.clone());
        core.run_op(APPLY).await.unwrap();
        fake.remove(old);
        let new = fake.add(new_pid, 20, 12.0);
        core.rescan(0.625).await.unwrap();
        assert_eq!(fake.size(new), 12.625);
        assert!(!core.zoomed.contains(&old));
        assert!(core.failed.is_empty());
        assert_eq!(fake.0.lock().unwrap().records.len(), 1);
        core.run_op(ZoomOp::Revert).await.unwrap();
        assert_eq!(fake.size(new), 12.0);
    }
}
#[tokio::test(start_paused = true)]
async fn reused_pid_never_reverts_the_previous_process_base() {
    let fake = Fake::default();
    let id = fake.add(1, 20, 14.625);
    fake.record(
        Identity {
            pid: 1,
            started_at_us: 10,
        },
        14.0,
        14.625,
    );
    ZoomCore::new(fake.clone())
        .run_op(ZoomOp::Revert)
        .await
        .unwrap();
    assert_eq!(fake.size(id), 14.625);
    assert_eq!(fake.0.lock().unwrap().log, ["1:delete"]);
}
#[tokio::test(start_paused = true)]
async fn exiting_mid_op_prunes_record_without_marking_failed() {
    let (logs, _guard) = capture_logs();
    let fake = Fake::default();
    fake.add(1, 10, 14.0);
    fake.0.lock().unwrap().vanish_on_set = true;
    let mut core = ZoomCore::new(fake.clone());
    core.run_op(APPLY).await.unwrap();
    assert!(core.failed.is_empty());
    assert!(core.zoomed.is_empty());
    assert_eq!(logs.count("WARN"), 0);
    assert_eq!(logs.count("instance exited mid-op"), 1);
    assert!(fake.0.lock().unwrap().records.is_empty());
}
#[tokio::test(start_paused = true)]
async fn failed_revert_keeps_recovery_record() {
    let fake = Fake::default();
    let id = fake.add(1, 10, 14.625);
    fake.record(id, 14.0, 14.625);
    fake.0.lock().unwrap().timeout_calls = IPC_MAX_ATTEMPTS;
    let mut core = ZoomCore::new(fake.clone());
    core.run_op(ZoomOp::Revert).await.unwrap();
    assert!(core.failed.contains(&id));
    assert_eq!(fake.0.lock().unwrap().records.len(), 1);
    core.run_op(ZoomOp::Revert).await.unwrap();
    assert_eq!(fake.size(id), 14.0);
}

#[tokio::test(start_paused = true)]
async fn lost_delta_change_retains_previous_target_for_reset_after_restart() {
    let fake = Fake::default();
    let id = fake.add(1, 10, 14.0);
    let mut core = ZoomCore::new(fake.clone());
    core.run_op(APPLY).await.unwrap();
    fake.0.lock().unwrap().drop_sets = IPC_MAX_ATTEMPTS;
    core.run_op(ZoomOp::Apply { delta_pt: 1.0 }).await.unwrap();
    assert_eq!(fake.size(id), 14.625);
    assert_eq!(
        fake.0.lock().unwrap().records[0].previous_target_pt,
        Some(14.625)
    );
    ZoomCore::new(fake.clone())
        .run_op(ZoomOp::Revert)
        .await
        .unwrap();
    assert_eq!(fake.size(id), 14.0);
}
#[tokio::test(start_paused = true)]
async fn crash_during_delta_change_recovers_either_delivery_outcome() {
    for current in [14.625, 15.0] {
        let fake = Fake::default();
        let id = fake.add(1, 10, current);
        fake.record(id, 14.0, 15.0);
        fake.0.lock().unwrap().records[0].previous_target_pt = Some(14.625);
        let mut core = ZoomCore::new(fake.clone());
        core.run_op(ZoomOp::Apply { delta_pt: 1.5 }).await.unwrap();
        assert_eq!(fake.size(id), 15.5);
        assert_eq!(fake.0.lock().unwrap().records[0].previous_target_pt, None);
        core.run_op(ZoomOp::Revert).await.unwrap();
        assert_eq!(fake.size(id), 14.0);
    }
}
