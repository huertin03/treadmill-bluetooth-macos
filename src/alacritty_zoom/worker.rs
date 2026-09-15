//! Sequential latest-state convergence, with scans only while zoomed.
use super::ipc::AlacrittyIpc;
use super::operations::ZoomCore;
use super::{Applied, INSTANCE_RESCAN_INTERVAL, ZoomConfig, ZoomOp, ZoomWant, plan};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::instrument::WithSubscriber;

#[derive(Clone)]
pub struct AlacrittyZoom {
    sender: watch::Sender<ZoomWant>,
}
impl AlacrittyZoom {
    /// Inert handle when optional zoom initialization fails; no task is spawned.
    pub fn disabled() -> Self {
        let (sender, _) = watch::channel(ZoomWant {
            config: ZoomConfig::default(),
            active: false,
        });
        Self { sender }
    }

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
        tokio::spawn(run_worker(ZoomCore::new(ipc), receiver).with_current_subscriber()),
    )
}

async fn run_worker<I: AlacrittyIpc>(
    mut core: ZoomCore<I>,
    mut receiver: watch::Receiver<ZoomWant>,
) {
    let mut applied = Applied::Unknown;
    let mut interval = tokio::time::interval(INSTANCE_RESCAN_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut retry_pending = false;
    let mut rescan_failed = false;
    loop {
        let want = *receiver.borrow_and_update();
        // A failed op may have partially changed instances. Even if a newer want
        // matches the last completed state, reconcile it once to repair those writes.
        let op = plan(applied, &want).or_else(|| {
            retry_pending.then(|| {
                if want.config.enabled && want.active {
                    ZoomOp::Apply {
                        delta_pt: want.config.delta_pt,
                    }
                } else {
                    ZoomOp::Revert
                }
            })
        });
        if let Some(op) = op {
            match core.run_op(op).await {
                Ok(()) => {
                    if retry_pending {
                        tracing::info!("Alacritty zoom reconciliation recovered");
                    }
                    retry_pending = false;
                    applied = match op {
                        ZoomOp::Revert => Applied::Base,
                        ZoomOp::Apply { delta_pt } => Applied::Zoomed { delta_pt },
                    };
                    interval.reset();
                    continue;
                }
                Err(error) => {
                    log_failure(retry_pending, &error, "reconciliation");
                    retry_pending = true;
                    interval.reset();
                    // A newer intent must not wait for the retry tick.
                    match receiver.has_changed() {
                        Ok(true) => continue,
                        Ok(false) => {}
                        Err(_) => {
                            tracing::info!("Alacritty zoom sender dropped; stopping worker");
                            return;
                        }
                    }
                }
            }
        }
        let scan_enabled = retry_pending || matches!(applied, Applied::Zoomed { .. });
        tokio::select! {
            biased;
            changed = receiver.changed() => {
                if changed.is_err() { tracing::info!("Alacritty zoom sender dropped; stopping worker"); return; }
            }
            _ = interval.tick(), if scan_enabled => {
                if retry_pending { continue; }
                if let Applied::Zoomed { delta_pt } = applied {
                    match core.rescan(delta_pt).await {
                        Ok(()) => {
                            if rescan_failed { tracing::info!("Alacritty zoom rescan recovered"); }
                            rescan_failed = false;
                        }
                        Err(error) => {
                            log_failure(rescan_failed, &error, "rescan");
                            rescan_failed = true;
                        }
                    }
                }
            }
        }
    }
}

fn log_failure(repeated: bool, error: &anyhow::Error, operation: &str) {
    if repeated {
        tracing::debug!(error = %format_args!("{error:#}"), operation, "Alacritty zoom operation still failing");
    } else {
        tracing::warn!(error = %format_args!("{error:#}"), operation, "Alacritty zoom operation failed; retry scheduled");
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
