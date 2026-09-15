use super::*;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);
struct TestDir(PathBuf);
impl TestDir {
    fn create() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join(format!("zoom-ipc-{}-{}", std::process::id(), NEXT_DIR.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn create_instance(&self, script: &str) -> AlacrittyInstance {
        let bin = self.0.join("alacritty");
        std::fs::write(&bin, format!("#!/opt/homebrew/bin/bash\nset -eu\n{script}\n")).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        AlacrittyInstance { identity: Identity { pid: 123, started_at_us: 1 }, socket: self.0.join("test.sock"), bin }
    }
}
impl Drop for TestDir { fn drop(&mut self) { std::fs::remove_dir_all(&self.0).unwrap(); } }

#[tokio::test]
async fn runner_passes_explicit_arguments_and_strips_inherited_environment() {
    const CHILD_FLAG: &str = "TM_ZOOM_IPC_TEST_CHILD";
    if std::env::var_os(CHILD_FLAG).is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "alacritty_zoom::ipc::tests::runner_passes_explicit_arguments_and_strips_inherited_environment", "--nocapture"])
            .env(CHILD_FLAG, "1").env("ALACRITTY_SOCKET", "stale-socket").env("ALACRITTY_WINDOW_ID", "999")
            .output().await.unwrap();
        assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        return;
    }
    assert!(std::env::var_os("ALACRITTY_SOCKET").is_some());
    let dir = TestDir::create();
    let instance = dir.create_instance(r#"
[[ -z ${ALACRITTY_SOCKET+x} && -z ${ALACRITTY_WINDOW_ID+x} ]]
[[ $1 == msg && $2 == -s && $5 == -w && $6 == -1 ]]
[[ $* != *--reset* ]]
printf '%s\n' "$@" >> "$3.argv"
if [[ $4 == get-config ]]; then printf '%s\n' '{"font":{"size":14.0}}'; fi
"#);
    let reply = run_call(&instance, &["get-config".into(), "-w".into(), "-1".into()]).await.unwrap();
    assert!(reply.success, "{}", reply.stderr);
    assert_eq!(crate::alacritty_zoom::parse_font_size(&reply.stdout), Some(14.0));
    let reply = run_call(&instance, &["config".into(), "-w".into(), "-1".into(), "font.size=14.625".into()]).await.unwrap();
    assert!(reply.success, "{}", reply.stderr);
    let argv = std::fs::read_to_string(dir.0.join("test.sock.argv")).unwrap();
    assert!(argv.contains(&format!("msg\n-s\n{}\nget-config\n-w\n-1\n", instance.socket.display())));
    assert!(argv.ends_with("config\n-w\n-1\nfont.size=14.625\n"));
}
#[tokio::test]
async fn timeout_kills_and_reaps_the_child() {
    let dir = TestDir::create();
    let instance = dir.create_instance("printf '%s' \"$$\" > \"$3.pid\"\nexec /bin/sleep 30");
    let error = run_call(&instance, &["get-config".into(), "-w".into(), "-1".into()]).await.unwrap_err();
    assert!(error.to_string().contains("timeout"));
    let pid: i32 = std::fs::read_to_string(dir.0.join("test.sock.pid")).unwrap().parse().unwrap();
    for _ in 0..100 {
        // SAFETY: signal zero only checks liveness of our own fake child.
        if unsafe { libc::kill(pid, 0) } == -1 { return; }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("timed-out child {pid} still exists");
}
#[test]
fn discovery_skips_dead_and_wrong_binary_without_removing_sockets_or_relogging() {
    let (logs, _guard) = crate::alacritty_zoom::test_support::capture_logs();
    let dir = TestDir::create();
    for name in [format!("Alacritty-{}.sock", std::process::id()), "Alacritty-2147483647.sock".into(), "irrelevant.sock".into()] {
        std::fs::write(dir.0.join(name), "").unwrap();
    }
    let mut skipped = HashSet::new();
    assert!(discover_instances(&dir.0, &mut skipped).unwrap().is_empty());
    assert_eq!(skipped.len(), 2);
    let before = skipped.clone();
    assert!(discover_instances(&dir.0, &mut skipped).unwrap().is_empty());
    assert_eq!(skipped, before);
    assert_eq!(logs.count("skipping orphan"), 2);
    assert_eq!(logs.count("WARN"), 0);
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 3);
    std::fs::remove_file(dir.0.join("Alacritty-2147483647.sock")).unwrap();
    discover_instances(&dir.0, &mut skipped).unwrap();
    assert_eq!(skipped.len(), 1);
}
