//! Crash recovery records; identity includes process start time to reject PID reuse.
use anyhow::Result;
use rusqlite::params;
use super::Store;

#[derive(Clone, Debug, PartialEq)]
pub struct ZoomRecord {
    pub pid: i32,
    pub started_at_us: i64,
    pub socket: String,
    pub base_pt: f64,
    pub target_pt: f64,
    /// Previous automated target during a pending delta change; cleared after verification.
    pub previous_target_pt: Option<f64>,
    pub applied_at_ms: i64,
}
impl Store {
    pub fn upsert_zoom_record(&self, record: &ZoomRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO alacritty_zoom (pid, started_at_us, socket, base_pt, target_pt, previous_target_pt, applied_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(pid) DO UPDATE SET started_at_us=excluded.started_at_us,
             socket=excluded.socket, base_pt=excluded.base_pt, target_pt=excluded.target_pt, previous_target_pt=excluded.previous_target_pt,
             applied_at_ms=excluded.applied_at_ms",
            params![record.pid, record.started_at_us, record.socket, record.base_pt, record.target_pt, record.previous_target_pt, record.applied_at_ms],
        )?;
        Ok(())
    }
    pub fn zoom_records(&self) -> Result<Vec<ZoomRecord>> {
        let mut stmt = self.conn.prepare("SELECT pid, started_at_us, socket, base_pt, target_pt, previous_target_pt, applied_at_ms FROM alacritty_zoom ORDER BY pid")?;
        Ok(stmt.query_map([], |row| Ok(ZoomRecord {
            pid: row.get(0)?, started_at_us: row.get(1)?, socket: row.get(2)?,
            base_pt: row.get(3)?, target_pt: row.get(4)?, previous_target_pt: row.get(5)?, applied_at_ms: row.get(6)?,
        }))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn delete_zoom_record(&self, pid: i32, started_at_us: i64) -> Result<()> {
        self.conn.execute("DELETE FROM alacritty_zoom WHERE pid=?1 AND started_at_us=?2", params![pid, started_at_us])?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persists_replaces_and_deletes_only_matching_identity() {
        let store = Store::open_at(std::path::Path::new(":memory:")).unwrap();
        let mut record = ZoomRecord { pid: 42, started_at_us: 100, socket: "test.sock".into(), base_pt: 14.0, target_pt: 14.625, previous_target_pt: Some(14.5), applied_at_ms: 200 };
        store.upsert_zoom_record(&record).unwrap();
        assert_eq!(store.zoom_records().unwrap(), vec![record.clone()]);
        record.started_at_us = 101;
        store.upsert_zoom_record(&record).unwrap();
        store.delete_zoom_record(42, 100).unwrap();
        assert_eq!(store.zoom_records().unwrap(), vec![record]);
        store.delete_zoom_record(42, 101).unwrap();
        assert!(store.zoom_records().unwrap().is_empty());
    }
}
