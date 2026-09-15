//! In-memory IPC/record fake. No filesystem discovery and no child processes.
use super::ipc::{AlacrittyInstance, AlacrittyIpc, Identity, Reply};
use crate::store::ZoomRecord;
use anyhow::{Result, bail};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct State {
    pub instances: Vec<AlacrittyInstance>,
    pub sizes: HashMap<Identity, f64>,
    pub records: Vec<ZoomRecord>,
    pub log: Vec<String>,
    pub drop_sets: usize,
    pub fail_sets: usize,
    pub empty_reads: usize,
    pub timeout_calls: usize,
    pub vanish_on_set: bool,
    pub scans: usize,
    pub call_delay: Option<std::time::Duration>,
}
#[derive(Clone, Default)]
pub struct Fake(pub Arc<Mutex<State>>);
impl Fake {
    pub fn add(&self, pid: i32, start: i64, size: f64) -> Identity {
        let identity = Identity {
            pid,
            started_at_us: start,
        };
        let mut state = self.0.lock().unwrap();
        state.instances.push(AlacrittyInstance {
            identity,
            socket: format!("/fake/Alacritty-{pid}.sock").into(),
            bin: "/fake/alacritty".into(),
        });
        state.sizes.insert(identity, size);
        identity
    }
    pub fn record(&self, identity: Identity, base: f64, target: f64) {
        self.0.lock().unwrap().records.push(ZoomRecord {
            pid: identity.pid,
            started_at_us: identity.started_at_us,
            socket: format!("/fake/Alacritty-{}.sock", identity.pid),
            base_pt: base,
            target_pt: target,
            previous_target_pt: None,
            applied_at_ms: 0,
        });
    }
    pub fn size(&self, identity: Identity) -> f64 {
        self.0.lock().unwrap().sizes[&identity]
    }
    pub fn remove(&self, identity: Identity) {
        self.0
            .lock()
            .unwrap()
            .instances
            .retain(|instance| instance.identity != identity);
    }
}
impl AlacrittyIpc for Fake {
    fn discover_instances(&mut self) -> Result<Vec<AlacrittyInstance>> {
        let mut state = self.0.lock().unwrap();
        state.scans += 1;
        Ok(state.instances.clone())
    }
    fn is_live(&mut self, instance: &AlacrittyInstance) -> bool {
        self.0
            .lock()
            .unwrap()
            .instances
            .iter()
            .any(|item| item.identity == instance.identity)
    }
    async fn call(&mut self, instance: &AlacrittyInstance, args: Vec<String>) -> Result<Reply> {
        let delay = self.0.lock().unwrap().call_delay;
        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
        assert!(!args.iter().any(|arg| arg == "--reset"));
        assert_eq!(&args[1..3], &["-w", "-1"]);
        let mut state = self.0.lock().unwrap();
        state
            .log
            .push(format!("{}:{}", instance.identity.pid, args.join(" ")));
        if state.timeout_calls > 0 {
            state.timeout_calls -= 1;
            bail!("Alacritty IPC timeout after 2s");
        }
        if args[0] == "get-config" {
            if state.empty_reads > 0 {
                state.empty_reads -= 1;
                return Ok(Reply {
                    success: true,
                    stdout: String::new(),
                    stderr: String::new(),
                });
            }
            return Ok(Reply {
                success: true,
                stdout: format!(
                    r#"{{"font":{{"size":{}}}}}"#,
                    state.sizes[&instance.identity]
                ),
                stderr: String::new(),
            });
        }
        if state.vanish_on_set {
            state
                .instances
                .retain(|item| item.identity != instance.identity);
            state.timeout_calls = 1;
            bail!("process exited");
        }
        if state.fail_sets > 0 {
            state.fail_sets -= 1;
            return Ok(Reply {
                success: false,
                stdout: String::new(),
                stderr: "BrokenPipe".into(),
            });
        }
        if state.drop_sets > 0 {
            state.drop_sets -= 1;
        } else {
            state.sizes.insert(
                instance.identity,
                args[3].strip_prefix("font.size=").unwrap().parse().unwrap(),
            );
        }
        Ok(Reply {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        })
    }
    fn zoom_records(&mut self) -> Result<Vec<ZoomRecord>> {
        Ok(self.0.lock().unwrap().records.clone())
    }
    fn upsert_zoom_record(&mut self, record: &ZoomRecord) -> Result<()> {
        let mut state = self.0.lock().unwrap();
        state.log.push(format!("{}:record", record.pid));
        state.records.retain(|item| item.pid != record.pid);
        state.records.push(record.clone());
        Ok(())
    }
    fn delete_zoom_record(&mut self, identity: Identity) -> Result<()> {
        let mut state = self.0.lock().unwrap();
        state.log.push(format!("{}:delete", identity.pid));
        state.records.retain(|record| {
            record.pid != identity.pid || record.started_at_us != identity.started_at_us
        });
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct Logs(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for Logs {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Logs {
    pub fn count(&self, text: &str) -> usize {
        String::from_utf8_lossy(&self.0.lock().unwrap())
            .matches(text)
            .count()
    }
}
pub fn capture_logs() -> (Logs, tracing::subscriber::DefaultGuard) {
    let logs = Logs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(move || writer.clone())
        .finish();
    (logs, tracing::subscriber::set_default(subscriber))
}
