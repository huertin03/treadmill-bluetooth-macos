//! Sequential latest-state convergence, with scans only while zoomed.
use super::ipc::AlacrittyIpc;
use super::operations::ZoomCore;
use super::{Applied, INSTANCE_RESCAN_INTERVAL, ZoomConfig, ZoomOp, ZoomWant, plan};
use tokio::sync::watch;
use tokio::task::JoinHandle;

#[derive(Clone)]
pub struct AlacrittyZoom {
    sender: watch::Sender<ZoomWant>,
}
impl AlacrittyZoom {
    pub fn set_active(&self, active: bool) {
        self.sender.send_if_modified(|want| {
            if want.active == active {
                return false;
            }
            want.active = active;
            true
        });
    }
    pub fn set_config(&self, config: ZoomConfig) {
        self.sender.send_if_modified(|want| {
            if want.config == config {
                return false;
            }
            want.config = config;
            true
        });
    }
}
/// The caller owns the join handle; dropping all senders ends the worker.
pub fn spawn_worker<I: AlacrittyIpc + 'static>(
    ipc: I,
    config: ZoomConfig,
) -> (AlacrittyZoom, JoinHandle<()>) {
    let (sender, receiver) = watch::channel(ZoomWant {
        config,
        active: false,
    });
    (
        AlacrittyZoom { sender },
        tokio::spawn(run_worker(ZoomCore::new(ipc), receiver)),
    )
}

async fn run_worker<I: AlacrittyIpc>(
    mut core: ZoomCore<I>,
    mut receiver: watch::Receiver<ZoomWant>,
) {
    let mut applied = Applied::Unknown;
    let mut interval = tokio::time::interval(INSTANCE_RESCAN_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let want = *receiver.borrow_and_update();
        if let Some(op) = plan(applied, &want) {
            if let Err(error) = core.run_op(op).await {
                tracing::warn!(%error, ?op, "Alacritty zoom reconciliation failed");
            }
            applied = match op {
                ZoomOp::Revert => Applied::Base,
                ZoomOp::Apply { delta_pt } => Applied::Zoomed { delta_pt },
            };
            interval.reset();
            // An update during IPC takes precedence over any rescan.
            continue;
        }
        tokio::select! {
            biased;
            changed = receiver.changed() => {
                if changed.is_err() { tracing::info!("Alacritty zoom sender dropped; stopping worker"); return; }
            }
            _ = interval.tick(), if matches!(applied, Applied::Zoomed { .. }) => {
                if let Applied::Zoomed { delta_pt } = applied && let Err(error) = core.rescan(delta_pt).await {
                    tracing::warn!(%error, "Alacritty zoom rescan failed");
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
