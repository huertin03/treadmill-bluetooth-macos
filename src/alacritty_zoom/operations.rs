//! Persist-before-write operations shared by the CLI and desired-state worker.
use std::collections::HashSet;
use anyhow::{Result, ensure};
use crate::store::ZoomRecord;
use super::ipc::{AlacrittyInstance, AlacrittyIpc, Identity};
use super::retry::{get_font_size, set_font_size};
use super::{ZoomOp, is_valid_delta, sizes_match};

pub struct ZoomCore<I> {
    pub ipc: I,
    pub zoomed: HashSet<Identity>,
    pub failed: HashSet<Identity>,
}
impl<I: AlacrittyIpc> ZoomCore<I> {
    pub fn new(ipc: I) -> Self { Self { ipc, zoomed: HashSet::new(), failed: HashSet::new() } }

    /// Reconcile persisted identities on every operation, including late-process scans.
    pub fn discover_and_prune(&mut self) -> Result<Vec<AlacrittyInstance>> {
        let instances = self.ipc.discover_instances()?;
        for record in self.ipc.zoom_records()? {
            let identity = record_identity(&record);
            if !instances.iter().any(|instance| instance.identity == identity) {
                tracing::debug!(?identity, "pruning dead Alacritty zoom record");
                self.ipc.delete_zoom_record(identity)?;
            }
        }
        let live: HashSet<_> = instances.iter().map(|instance| instance.identity).collect();
        self.zoomed.retain(|identity| live.contains(identity));
        self.failed.retain(|identity| live.contains(identity));
        Ok(instances)
    }

    pub async fn run_op(&mut self, op: ZoomOp) -> Result<()> {
        self.zoomed.clear();
        self.failed.clear();
        let instances = self.discover_and_prune()?;
        if instances.is_empty() { tracing::debug!("Alacritty not running"); }
        self.run_instances(instances, op).await
    }

    #[allow(dead_code)] // Called by the P2 worker.
    pub async fn rescan(&mut self, delta_pt: f64) -> Result<()> {
        let instances = self.discover_and_prune()?.into_iter()
            .filter(|instance| !self.zoomed.contains(&instance.identity) && !self.failed.contains(&instance.identity)).collect();
        self.run_instances(instances, ZoomOp::Apply { delta_pt }).await
    }

    async fn run_instances(&mut self, instances: Vec<AlacrittyInstance>, op: ZoomOp) -> Result<()> {
        let mut outcomes = Vec::new();
        for instance in instances {
            match self.run_instance(&instance, op).await {
                Ok(Some((base, target))) => {
                    if matches!(op, ZoomOp::Apply { .. }) { self.zoomed.insert(instance.identity); }
                    outcomes.push((instance.identity.pid, base, target));
                }
                Ok(None) => {},
                Err(error) => self.handle_failure(&instance, error)?,
            }
        }
        if !outcomes.is_empty() { tracing::info!(?op, ?outcomes, "Alacritty zoom operation completed (pid, base, target)"); }
        Ok(())
    }

    fn handle_failure(&mut self, instance: &AlacrittyInstance, error: anyhow::Error) -> Result<()> {
        if !self.ipc.is_live(instance) {
            tracing::debug!(identity = ?instance.identity, %error, "Alacritty instance exited mid-op");
            self.ipc.delete_zoom_record(instance.identity)?;
            return Ok(());
        }
        tracing::warn!(identity = ?instance.identity, %error, "Alacritty zoom operation failed");
        self.failed.insert(instance.identity);
        Ok(())
    }

    async fn run_instance(&mut self, instance: &AlacrittyInstance, op: ZoomOp) -> Result<Option<(f64, f64)>> {
        let record = self.ipc.zoom_records()?.into_iter().find(|record| record_identity(record) == instance.identity);
        match op {
            ZoomOp::Apply { delta_pt } => self.apply_instance(instance, record, delta_pt).await.map(Some),
            ZoomOp::Revert => {
                let Some(record) = record else { return Ok(None); };
                self.revert_instance(instance, &record).await
            }
        }
    }

    async fn apply_instance(&mut self, instance: &AlacrittyInstance, record: Option<ZoomRecord>, delta: f64) -> Result<(f64, f64)> {
        ensure!(is_valid_delta(delta), "invalid zoom delta: {delta}; expected 0 < pt <= 8");
        let current = get_font_size(&mut self.ipc, instance).await?;
        let owned = record.filter(|record| matches_record(current, record));
        let base = owned.as_ref().map_or(current, |record| record.base_pt);
        let target = base + delta;
        ensure!(target.is_finite(), "non-finite Alacritty font target: base={base}, delta={delta}");
        let mut pending = ZoomRecord {
            pid: instance.identity.pid, started_at_us: instance.identity.started_at_us,
            socket: instance.socket.to_string_lossy().into_owned(), base_pt: base, target_pt: target,
            previous_target_pt: owned.filter(|_| !sizes_match(current, target)).map(|_| current),
            applied_at_ms: chrono::Utc::now().timestamp_millis(),
        };
        self.ipc.upsert_zoom_record(&pending)?;
        set_font_size(&mut self.ipc, instance, target).await?;
        if pending.previous_target_pt.take().is_some() { self.ipc.upsert_zoom_record(&pending)?; }
        Ok((base, target))
    }

    async fn revert_instance(&mut self, instance: &AlacrittyInstance, record: &ZoomRecord) -> Result<Option<(f64, f64)>> {
        let current = get_font_size(&mut self.ipc, instance).await?;
        let changed = matches_record(current, record);
        if changed {
            set_font_size(&mut self.ipc, instance, record.base_pt).await?;
        } else {
            tracing::info!(pid = instance.identity.pid, current, recorded_target = record.target_pt, "Alacritty font changed externally; skipping revert");
        }
        self.ipc.delete_zoom_record(instance.identity)?;
        Ok(changed.then_some((record.target_pt, record.base_pt)))
    }
}
pub fn matches_record(current: f64, record: &ZoomRecord) -> bool {
    sizes_match(current, record.target_pt) || record.previous_target_pt.is_some_and(|pt| sizes_match(current, pt))
}
pub fn record_identity(record: &ZoomRecord) -> Identity { Identity { pid: record.pid, started_at_us: record.started_at_us } }

#[cfg(test)]
#[path = "operations_tests.rs"]
mod tests;
