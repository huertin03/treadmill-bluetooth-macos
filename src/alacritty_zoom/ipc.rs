//! Native discovery and bounded CLI calls. Never connect merely to probe a socket.
use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use anyhow::{Context, Result, ensure};
use tokio::process::Command;
use crate::store::{Store, ZoomRecord};
use super::{IPC_CALL_TIMEOUT, parse_socket_pid};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Identity { pub pid: i32, pub started_at_us: i64 }
#[derive(Clone, Debug)]
pub struct AlacrittyInstance { pub identity: Identity, pub socket: PathBuf, pub bin: PathBuf }
#[derive(Debug)]
pub struct Reply { pub success: bool, pub stdout: String, pub stderr: String }

/// One-call seam: retry and recovery logic is shared by production and fake clients.
pub trait AlacrittyIpc: Send {
    fn discover_instances(&mut self) -> Result<Vec<AlacrittyInstance>>;
    fn is_live(&mut self, instance: &AlacrittyInstance) -> bool;
    fn call(&mut self, instance: &AlacrittyInstance, args: Vec<String>) -> impl Future<Output = Result<Reply>> + Send;
    fn zoom_records(&mut self) -> Result<Vec<ZoomRecord>>;
    fn upsert_zoom_record(&mut self, record: &ZoomRecord) -> Result<()>;
    fn delete_zoom_record(&mut self, identity: Identity) -> Result<()>;
}

pub struct SystemIpc { dir: PathBuf, store: Store, skipped: HashSet<i32> }
impl SystemIpc {
    pub fn new(dir: PathBuf, store: Store) -> Self { Self { dir, store, skipped: HashSet::new() } }
}
impl AlacrittyIpc for SystemIpc {
    fn discover_instances(&mut self) -> Result<Vec<AlacrittyInstance>> {
        discover_instances(&self.dir, &mut self.skipped)
    }
    fn is_live(&mut self, instance: &AlacrittyInstance) -> bool {
        read_process(instance.identity.pid).is_some_and(|(identity, bin)| identity == instance.identity && bin == instance.bin)
    }
    async fn call(&mut self, instance: &AlacrittyInstance, args: Vec<String>) -> Result<Reply> {
        ensure!(self.is_live(instance), "Alacritty instance exited or identity changed before IPC");
        run_call(instance, &args).await
    }
    fn zoom_records(&mut self) -> Result<Vec<ZoomRecord>> { self.store.zoom_records() }
    fn upsert_zoom_record(&mut self, record: &ZoomRecord) -> Result<()> { self.store.upsert_zoom_record(record) }
    fn delete_zoom_record(&mut self, identity: Identity) -> Result<()> { self.store.delete_zoom_record(identity.pid, identity.started_at_us) }
}

pub fn discover_instances(dir: &Path, skipped: &mut HashSet<i32>) -> Result<Vec<AlacrittyInstance>> {
    let mut instances = Vec::new();
    let mut seen = HashSet::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("read socket directory {}", dir.display()))? {
        let entry = entry.context("read socket directory entry")?;
        let Some(pid) = entry.file_name().to_str().and_then(parse_socket_pid) else { continue; };
        seen.insert(pid);
        if let Some((identity, bin)) = read_process(pid) {
            skipped.remove(&pid);
            instances.push(AlacrittyInstance { identity, bin, socket: entry.path() });
        } else if skipped.insert(pid) {
            tracing::debug!(pid, "skipping orphan or non-Alacritty socket");
        }
    }
    skipped.retain(|pid| seen.contains(pid));
    Ok(instances)
}

fn read_process(pid: i32) -> Option<(Identity, PathBuf)> {
    use std::os::unix::ffi::OsStrExt;
    let mut path = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: buffers are correctly sized and remain valid for both kernel calls.
    let size = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
    if size <= 0 { return None; }
    let end = path.iter().position(|byte| *byte == 0)?;
    let bin = PathBuf::from(std::ffi::OsStr::from_bytes(&path[..end]));
    if bin.file_name()? != "alacritty" { return None; }
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: proc_pidinfo initializes the full struct only on a full-size return.
    let read = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, info.as_mut_ptr().cast(), size) };
    if read != size { return None; }
    // SAFETY: full initialization was checked above.
    let info = unsafe { info.assume_init() };
    let started_at_us = info.pbi_start_tvsec.checked_mul(1_000_000)?.checked_add(info.pbi_start_tvusec)?;
    Some((Identity { pid, started_at_us: i64::try_from(started_at_us).ok()? }, bin))
}

fn build_command(instance: &AlacrittyInstance, args: &[String]) -> Command {
    let mut command = Command::new(&instance.bin);
    command.arg("msg").arg("-s").arg(&instance.socket).args(args)
        .env_remove("ALACRITTY_SOCKET").env_remove("ALACRITTY_WINDOW_ID")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    command
}

pub async fn run_call(instance: &AlacrittyInstance, args: &[String]) -> Result<Reply> {
    ensure!(!args.iter().any(|arg| arg == "--reset"), "font-only IPC forbids --reset");
    let output = tokio::time::timeout(IPC_CALL_TIMEOUT, build_command(instance, args).output())
        .await.context("Alacritty IPC timeout after 2s")?.context("spawn/read Alacritty client")?;
    Ok(Reply { success: output.status.success(), stdout: String::from_utf8_lossy(&output.stdout).into_owned(), stderr: String::from_utf8_lossy(&output.stderr).into_owned() })
}

#[cfg(test)]
#[path = "ipc_tests.rs"]
mod tests;
