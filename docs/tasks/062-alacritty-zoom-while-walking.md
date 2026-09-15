# 062 — Alacritty font zoom while walking (`alacritty_zoom`)

**Status:** P2 implemented, live smoke pending (2026-09-15). Line numbers below predate task 063 (landed the same day, touches
`session.rs`/`commands`): re-locate by symbol. Operator confirmed: font-only revert and its costs (D3/D4), shrink only on `Paused` or session end, and a manual IPC check (grow/shrink both ways, other overrides survive, manual-zoom guard). Facts and sources:
research [008](../research/008-alacritty-ipc-font-size.md). Read it first. Everything below is
derived from it.

## P1 implementation notes (2026-09-15)

- P1 implemented the core, desired-state worker, SQLite recovery, config loader
  and CLI. P2 now wires the worker into daemon startup, presence, session end,
  and config hot reload; direct CLI calibration/recovery remains available.
- Recovery refinement to D3/D4: the table also has nullable `previous_target_pt`.
  During a delta change, persist the previous automated size alongside the new
  target before writing. Clear it after verified delivery. Until then, either
  size matches the record for recovery/base selection. Otherwise a crash or five
  lost sets between replacing the record and applying the new target would lose
  the original base and make reset skip our previous zoom as an external change.
- Revert failures retain their record for a later explicit operation or restart;
  a successful revert or confirmed external change deletes it. IPC checks the
  full process identity before each call, including queued instances.
- Fake IPC tests cover retries, restart/PID reuse, interrupted delta changes,
  coalescing, late processes, failed-pid suppression and log levels. Runner tests
  use disposable fake executables under `target/`; gate runs redirect `TMPDIR`
  there as well, isolating existing tests from the host's real temporary directory.
- Validation: `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo build`, `cargo test` passed in that order; **281 tests passed**, including
  26 Alacritty zoom tests. The intentional schema snapshot includes the new table.
- P2 resolves overlapping CLI/worker operations with D14's shared advisory lock.
  Whole-op failures preserve the last completed state and retry on interval ticks;
  each error streak warns once, repeats at DEBUG, and logs recovery once at INFO.
- P2 includes nonfatal zoom startup, presence/session wiring, config hot reload,
  and best-effort uninstall reset. Manual live smoke and dotfiles changes remain
  orchestrator work. No live Alacritty is invoked by the tests.

## Context

The operator reads the terminal while walking. Eyes move, so small text is tiring, and today they
press ⌘= twice in Alacritty at the start of a walk and ⌘0 after it. Automate that:

- **Zoom in when walking starts, zoom out only when the walk is really over** (operator decision
  2026-09-15):
  - **Grow** on entering presence `Walking` (belt moving + steps growing), from `Unknown`, `Paused` or
    `AwayWhileRunning`.
  - **Shrink** only on
    - `Paused` (belt stopped by any means: the daemon's auto-pause, `tm stop`, the remote, the console), or
    - the end of the treadmill BLE session (by any path: out of range, power-off, deliberate disconnect,
      error).
  - **`AwayWhileRunning` changes nothing.** The operator stepped off, but the belt keeps running, so the
    font stays big. If nobody returns, auto-pause (`auto_pause_minutes`) stops the belt → `Paused` → shrink.
- **Alacritty only.** No Alacritty process running → silently skip. Ghostty, WezTerm and every other
  terminal are never touched.
- **Base size is read from Alacritty itself** (`get-config`, which reflects `~/.alacritty.toml`).
  The walking size is base + a configured pt delta.
- Configurable in `config.toml` and via a `tm` CLI command (on/off at minimum).

## Verified constraints (research 008)

1. **Runtime zoom cannot be read.** ⌘= changes a display-only field that IPC never
   reports. So "read the target from the current window" is impossible. The delta
   must be configured. Two ⌘= presses on the operator's Retina displays at base 14 pt
   equal exactly **+0.625 pt** (14.625), which becomes the default.
2. **Manual zoom blocks automation.** Alacritty ignores a config size change on a window whose
   size was changed with ⌘=/⌘-, until ⌘0. That is Alacritty behaviour we cannot bypass.
   Consequence: the operator stops using ⌘= for walking. "Manual wins" is acceptable
   and must be documented in the CLI status hint.
3. **Alacritty 0.17 IPC on macOS is lossy** (research 008 fact 8). The server accepts on a non-blocking
   listener, and on macOS the accepted socket inherits `O_NONBLOCK`. If the client's bytes have not
   arrived yet, `read_line` returns `WouldBlock` and the server silently drops the connection.
   - Observed by the operator: `Error: Os { code: 57, NotConnected }` / `code: 32, BrokenPipe` on about
     4 of 10 `msg config` calls, and the size was **not** applied.
   - Observed in a 60-call loop: 1 silent loss with **exit 0**.
   - So the exit code is meaningless in both directions: `msg config` exits 0 also for a rejected option,
     and a lost `get-config` prints nothing with exit 0.
   - **Every set must be verified by a `get-config` read-back and retried** (D3).
4. `-w -1` overrides **accumulate** in Alacritty (unbounded `Vec`). Only `--reset -w -1` clears them,
   and it clears **all** runtime keys, not just the font. The operator rejected that (D3), so a key
   cannot be un-set, only overwritten with the base value.
5. `get-config -w -1` includes active `-w -1` overrides, so it cannot tell the file base from our own
   override. After a crash, a naive "read base, add delta" would compound. D4 records solve this.
6. Sockets are `$TMPDIR/Alacritty-<pid>.sock`, and there are orphans after crashes (13 on the host). Plain
   `alacritty msg` without `-s` reaches **one arbitrary** process. The daemon's `TMPDIR` under launchd
   equals Alacritty's (verified), and both use Rust `std::env::temp_dir()`.
7. The daemon's `PATH` has no Homebrew. Inherited `ALACRITTY_SOCKET` / `ALACRITTY_WINDOW_ID`
   can be stale in `tm` run from a shell.
8. A hung Alacritty hangs `get-config` (it waits for a reply). The daemon has a fail-fast panic
   hook (`src/daemon/run_loop.rs:150-163`, exit 101): **any panic in new code kills the daemon.**

## Design decisions

- **D1 — Mechanism: the `alacritty msg` CLI, per live process.** Do not reimplement the JSON socket
  protocol (undocumented, no version field). The client binary is the **running process's own
  executable** (`libc::proc_pidpath(pid)`), which avoids a PATH lookup, needs no config key, and
  keeps the version matched. Always pass `-s <socket>` and `-w -1` explicitly and
  `env_remove("ALACRITTY_SOCKET")` / `env_remove("ALACRITTY_WINDOW_ID")` on the child.
- **D2 — Instance discovery without connecting.** `read_dir(std::env::temp_dir())`, then:
  - keep names matching `Alacritty-<pid>.sock`;
  - `proc_pidpath(pid)` must succeed and have basename `alacritty`, otherwise skip at DEBUG
    (orphan, or pid reused by another program).
  
  Never connect to a socket to probe it, and never delete orphans (they belong to Alacritty, whose own client
  cleans them). The socket dir is injectable for tests.
- **D3 — Operations touch `font.size` only; `--reset` is NEVER sent** (operator decision 2026-09-15:
  other runtime overrides must survive). Each child call: `tokio::process::Command`,
  `kill_on_drop(true)`, stdin null, stdout/stderr captured,
  `tokio::time::timeout(IPC_CALL_TIMEOUT = 2s)`. Sizes compare with tolerance `SIZE_EPSILON = 1e-3`,
  because Alacritty serialises pt through `f32` (`14.7` reads back as `14.6999998…`).
  - `Apply{delta}` per instance:
    1. `msg -s S get-config -w -1` → current size `cur`.
    2. Find the base. If a zoom record for this pid exists and `cur ≈ record.target` (or `previous_target_pt`), we already
       zoomed it: `base = record.base`. Otherwise `base = cur`. Reading the base this way is
       crash-safe, so constraint 5 does not apply.
    3. `target = base + delta`.
    4. **Persist the record `{pid, socket, base, target}` first** (D4).
    5. `msg -s S config -w -1 font.size=<fmt(target)>`.
    6. Verify-and-retry (below) until `get-config` reads `≈ target`.
  - `Revert` per **recorded** instance (never an unrecorded one, so windows we did not zoom are
    never touched):
    1. `get-config -w -1` → `cur`.
    2. If `cur ≈ record.target` (or `previous_target_pt`), send `config -w -1 font.size=<fmt(record.base)>` and verify-and-retry.
    3. If not, someone else changed it or the pid was reused: skip at INFO.
    4. Delete the record after successful delivery or confirmed external change.
       Failed delivery retains it for recovery.
  - **Verify-and-retry** (constraint 3):
    - one *attempt* = `config … font.size=X` (exit code only logged at DEBUG), then `get-config` → size;
    - success when `≈ X`;
    - on mismatch, non-zero exit, empty or unparsable `get-config` stdout, or timeout → retry after
      backoff `IPC_RETRY_BACKOFF = [50, 100, 200, 400] ms`, up to `IPC_MAX_ATTEMPTS = 5`;
    - a `get-config` in step 1 that comes back empty is retried the same way;
    - absolute `font.size=X` is idempotent, so a duplicate delivery is harmless (one extra override entry);
    - `WARN` only when all attempts are exhausted (log attempts, last stderr, last read size);
    - DEBUG per retry;
    - the backoff uses `tokio::time::sleep`, never blocking.
  - Format the size with ≤3 decimals, trailing zeros trimmed (`14.625`, `15`).
- **D4 — Zoom records in SQLite** (`Store`, new table `alacritty_zoom`:
  `pid INTEGER PRIMARY KEY, started_at_us INTEGER NOT NULL, socket TEXT NOT NULL, base_pt REAL NOT NULL,
  target_pt REAL NOT NULL, previous_target_pt REAL, applied_at_ms INTEGER NOT NULL`). A record matches an instance only when
  **both** `pid` and `started_at_us` match (D13). This is the memory that makes a font-only revert possible after a
  daemon crash or restart: `get-config` cannot tell the file base from our override (constraint 5).
  - `previous_target_pt` is an endorsed recovery field: persist the previous
    automated size before replacing a target, accept either target when choosing
    the recorded base or reverting, and clear it only after verified delivery.
  - It is shared safely by the daemon and the `tm` CLI under D14's operation lock.
  - Rows are deleted on revert, and rows whose pid is no longer a live `alacritty` get pruned on every
    op. The table is bounded by the number of live Alacritty processes.
  - Accepted costs, told to the operator:
    - (a) After the first walk, the running Alacritty process holds a `font.size=<base>` override.
      Editing `font.size` in `.alacritty.toml` then needs an Alacritty restart to show. Other keys
      still live-reload.
    - (b) Each walk cycle appends two entries to Alacritty's in-process override list (research 008
      fact 4). That is negligible and cleared on Alacritty restart.
- **D5 — Desired-state worker, not fire-and-forget calls.** One long-lived tokio task spawned in
  `daemon::run_loop::run()` receives a `tokio::sync::watch` of `ZoomWant { config: ZoomConfig,
  active: bool }` (`active` is the walk latch from D8, not raw presence) and converges to the **latest** value sequentially:
  - ordering races (walk/stop flapping) are impossible by construction;
  - flapping coalesces;
  - the event loop never waits on a child process.
  
  The handle `AlacrittyZoom` wraps the `watch::Sender` and exposes `set_active(bool)` and
  `set_config(ZoomConfig)`, both via `send_if_modified`.
- **D6 — Pure planner** `plan(applied: Applied, want: &ZoomWant) -> Option<ZoomOp>`, where
  `Applied = Unknown | Base | Zoomed { delta_pt }` and `ZoomOp = Revert | Apply { delta_pt }`.

  | applied | want | op |
  |---|---|---|
  | `Unknown` | not (enabled ∧ active) | `Revert` (startup reconcile; touches recorded pids only, so it is a no-op when nothing was zoomed, even with the feature off) |
  | `Unknown`/`Base` | enabled, active | `Apply` |
  | `Base` | disabled or not active | none |
  | `Zoomed{d}` | enabled, active, same `d` | none |
  | `Zoomed{d}` | enabled, active, other `d` | `Apply` (pt changed via hot-reload; D3 step 2 keeps the recorded base) |
  | `Zoomed{_}` | disabled or not active | `Revert` |

  After a successful op: `Revert` → `Base`, `Apply` → `Zoomed`. Whole-op errors
  (including record IO and lock timeout) keep the previous state and retry on the
  next rescan tick, even while Base/Unknown. Newer desired state wins; retry still
  reconciles partial writes when it matches the previous completed state.
  - `Apply` runs on **every** discovered instance; `Revert` runs on every recorded live instance.
  - The worker tracks `zoomed_pids` and `failed_pids`. Per-instance failure → `WARN`, move on. A failed
    pid is not retried until the next op (no WARN spam).
  - "No Alacritty running" → DEBUG, and the state still advances. That is what makes the next point work.
- **D7 — Late processes.** While `Applied::Zoomed`, a `tokio::time::interval(INSTANCE_RESCAN_INTERVAL
  = 2s)` arm re-discovers instances and runs `Apply` only on instances (identity per D13) not in
  `zoomed ∪ failed`.
  - The rescan is cheap: one `read_dir` of `$TMPDIR` (~600 entries on the host), a name filter first, and
    then `proc_pidpath`/`proc_pidinfo` only for the ~20 `Alacritty-*.sock` names.
  - No late-process rescan while `Base`; a pending whole-op retry still arms the tick.
  This covers Alacritty (re)launched mid-walk; orphan sockets show it does crash. New windows inside
  a zoomed process inherit `-w -1` by themselves (research 008 fact 3).
  - Memory rule: a `select!` `if` guard does **not** stop the arm's future expression from being built on
    every pass. `interval.tick()` is safe to build; do not put `unwrap`s or state-dependent
    construction in an arm expression.
  - `watch::Receiver::changed()` returning `Err` (sender dropped) → INFO, end the task.
- **D13 — Operator restarts Alacritty (not herdr) — a routine case, not an edge.**
  - Evidence 2026-09-15: pid 48251 → 79663 within the day, and orphan sockets grew 13 → 19. The
    operator's restarts do **not** exit cleanly: the socket's `Drop` does not run, so every restart
    leaves an orphan.
  - The herdr server survives, and `herdr-bar` re-attaches in the new window (the pty resize is the same as for ⌘=).
  - **Instance identity = `(pid, start time)`.** The start time comes from
    `libc::proc_pidinfo(pid, PROC_PIDTBSDINFO)` → `pbi_start_tvsec`/`pbi_start_tvusec` as microseconds.
    A reused pid can never inherit a record, and `zoomed`/`failed` are keyed the same way.
  - **Restart while active (walking or stepped off with the belt running):**
    - the new process starts at the file base (14);
    - the D7 rescan finds the new identity within ≤2 s and runs `Apply`;
    - if the socket appears before the first window exists, the `-w -1` override lands in
      `global_ipc_options`, and the window opens already zoomed (research 008 fact 3). Otherwise it
      opens at 14 and grows ≤2 s later;
    - IPC calls to a process still starting may be lost or time out, and verify-and-retry (D3) covers that.
  - **Restart while not active:** nothing to do. The new process is at base, and the restart even clears the
    `font.size` pin (D4 cost a) and any manual ⌘= zoom.
  - **Old records:** the dead identity is pruned on the next op or rescan. It is never reverted, since there is
    nothing to revert.
  - **Quit during an op:** calls to that instance fail or time out. Before logging, re-check liveness with
    `proc_pidpath` + start time. If the process is gone → DEBUG `instance exited mid-op`, prune its
    record, and do **not** mark it failed or WARN. Only a still-live instance that exhausts its retries gets
    the WARN.
  - **Orphan sockets keep piling up** (Alacritty's own file, D2: never deleted by us). Discovery must stay
    quiet and cheap with dozens of them: DEBUG at most once per identity, not per rescan.
- **D8 — Triggers (daemon wiring, thin).**
  - `run()` (`src/daemon/run_loop.rs:170`, next to `live_config` at :183-194): load the config, spawn the
    worker with `active=false`. The first convergence is the startup `Revert` of recorded pids.
  - Presence transition (`src/daemon/session.rs:244-247`, right after
    `state.presence_state = …`, before the `match`): `if let Some(active) =
    alacritty_zoom::zoom_intent(next_state) { zoom.set_active(active) }`. The pure `zoom_intent` returns:
    - `Walking` → `Some(true)`;
    - `Paused` → `Some(false)`;
    - `AwayWhileRunning` / `Unknown` → `None` (keep the latch).
    
    Accepted edge: if the daemon restarts while the operator is stepped off with the belt running, the
    startup `Revert` shrinks the font, and it grows again on the next `Walking`.
  - Session end (`src/daemon/run_loop.rs:353`, next to `notify::treadmill_lost()`, **before** the
    possibly-hanging BLE disconnect): `zoom.set_active(false)`.
  - Hot reload (`src/config_apply.rs`): add `alacritty_zoom: ZoomConfig` to `LiveConfig` (:18),
    `ConfigDelta` (:33, `is_empty` :44), `diff` (:113), `reload_if_changed` (:152) and
    `apply_config` (:165), plus a new `ConfigEffect::AlacrittyZoomChanged`. Its executor
    (`src/daemon/config.rs:24-116`, or directly at the call site `src/daemon/session.rs:488-517`,
    whichever is thinner) logs old→new and calls `zoom.set_config(new)`. Hot reload runs only while a
    treadmill session is live. That is fine: an idle change needs no action, the CLI `off` reverts by itself, and
    the first `config_tick` of a new session force-reloads (`goals_mtime = None`, session.rs:117).
  - `stream_with_presence` gains a `zoom: &AlacrittyZoom` parameter (clippy `too_many_arguments` is
    already allowed there).
  - `src/config_apply.rs` is 970 lines (orange zone): add only the field plumbing and tests there, no logic.
  - **No SIGTERM handler** (non-goal). A crash, watchdog exit or reinstall leaves the zoom until the next
    daemon start (launchd KeepAlive, ≤10 s) → startup `Revert`. Uninstall calls the CLI `reset` (D11).
- **D9 — Config (top-level keys, `src/config/alacritty_zoom.rs`, pattern of `src/config/show_speed.rs`).**
  - `alacritty_zoom = true|false`, default `false` (opt-in).
  - `alacritty_zoom_pt = <float>`, default `0.625`, valid `0 < pt ≤ MAX_ZOOM_PT (8.0)` and finite.
  - Absent → quiet default; invalid → `WARN` + default (same style as `show_speed`).
  - Returns `ZoomConfig { enabled: bool, delta_pt: f64 }` (`PartialEq`).
  - Re-export in `src/config/mod.rs`.
  - Written with the existing `config::upsert_top_level_key` (`src/config/file.rs:106`, inserts
    before the first `[section]`).
  - Document both keys, commented out, in `config/config.example.toml` after `led_on_connect` (:34), including
    the ⌘= caveat.
- **D10 — CLI `tm alacritty-zoom [on|off|pt <value>|preview|reset]`** (`src/commands/alacritty_zoom.rs`,
  clap next to `SpeedWidget` in `src/main.rs:178-216`, dispatch next to :316 / no-BLE group :364-368;
  pattern `src/widget.rs:15-39`, `src/commands/led.rs`). Values configurable by the operator are cyan via
  `commands::common::highlight_config` (задача 057).
  - Without an argument, status:
    - `alacritty zoom: on (+0.625 pt)`;
    - a live probe `alacritty: running pid 48251 — base 14 pt → walking 14.625 pt`, or
      `alacritty: not running`;
    - the hint line `a window zoomed by hand (⌘=/⌘-) ignores automation until ⌘0`.
  - `on` / `off`: upsert `alacritty_zoom`. `off` **also runs `Revert` directly** (recorded pids), so
    a stopped daemon cannot leave the terminal zoomed.
  - `pt <value>`: validate like the loader, then upsert `alacritty_zoom_pt`. Invalid → error, non-zero exit.
  - `preview`: `Apply` with the configured pt **now**, regardless of `enabled` (for calibrating the delta).
    Print the target and note that the daemon re-converges on the next presence transition.
  - `reset`: `Revert` now (recorded pids only; `font.size` only).
  - The CLI talks to Alacritty directly. There is no single-owner rule here (unlike BLE), so no daemon queue.
- **D11 — Surroundings.**
  - `tm status` (`src/commands/status.rs`, config block ~:258-266): one read-time line
    `alacritty zoom: on (+0.625 pt)` / `off`, read from the file like `workout_gap_minutes`. No
    `daemon_status` snapshot (YAGNI).
  - `scripts/uninstall-daemon.sh`: after unloading the agent and **before** removing the `tm` link,
    if the config has `alacritty_zoom = true` (grep), run the linked binary's `alacritty-zoom reset`,
    best-effort (`|| true`).
  - Dependencies: `serde_json` (parse `get-config`) and the tokio feature `process`.
- **D12 — Logging.**
  - INFO once per op outcome (`alacritty zoom applied base=14 target=14.625 pids=[…]` /
    `reverted`), no per-tick logs.
  - `WARN` for spawn error, timeout, non-zero exit (include stderr), unparsable `get-config`,
    read-back mismatch and invalid config.
  - DEBUG for orphan sockets and "not running".

- **D14 — Cross-process operation lock.** Every core operation (`run_op`, `rescan`,
  including CLI preview/reset/off) owns an exclusive advisory lock on
  `~/Library/Application Support/treadmill-bluetooth-macos/alacritty_zoom.lock`.
  The lock shares `Store::db_path` directory resolution; test paths are injectable.
  - Use `libc::flock(fd, LOCK_EX | LOCK_NB)`. On EWOULDBLOCK, sleep asynchronously
    for 50 ms and retry until `ZOOM_LOCK_WAIT = 40s`; timeout returns an error.
    No blocking flock on Tokio threads. Worker retries whole-op errors; CLI exits
    nonzero with the error. Each operation re-reads records and get-config while locked.
  - Create the file with mode 0600. A RAII guard owns its File; dropping it or
    process exit releases the kernel lock. Never delete the production lock file.
  - Status tries the lock once. If busy, print `daemon operation in progress` and
    probe read-only, without pruning/deleting records.
  - Tests use disposable directories under `target/`, covering serialization,
    timeout, and reset waiting for an in-flight apply before restoring its base.

## Plan

### P1 — core + CLI (no daemon wiring)

1. `Cargo.toml`: `serde_json = "1"`, tokio feature `"process"`.
2. `src/alacritty_zoom/mod.rs` (pure): `ZoomConfig`, `ZoomWant`, `Applied`, `ZoomOp`, `plan`,
   `parse_socket_pid(&str) -> Option<i32>`, `parse_font_size(json: &str) -> Option<f64>`,
   `format_font_size(f64) -> String`, constants.
3. `src/alacritty_zoom/ipc.rs`: `AlacrittyInstance { pid, socket: PathBuf, bin: PathBuf }`;
   `discover_instances(dir: &Path)` (D2); a runner that executes one `msg` call with the D1/D3 rules;
   `get_font_size(&inst) -> Result<f64>`, `set_font_size(&inst, pt) -> Result<()>`. Revert/apply orchestration with the D4 records lives in the worker/core, not in the runner. Put a trait over
   discovery + calls (`AlacrittyIpc`, async fn in trait, used generically, not `dyn`) so the worker is
   testable with a fake.
4. `src/alacritty_zoom/worker.rs`: `AlacrittyZoom` handle + `spawn_worker` + a convergence loop generic
   over the trait (D5–D7). No `unwrap`/`expect` outside tests.
5. `src/config/alacritty_zoom.rs` loader (D9), `config/config.example.toml`.
5a. `Store` table `alacritty_zoom` (D4): `CREATE TABLE IF NOT EXISTS` in the existing schema setup under
   `src/store/`, plus `upsert_zoom_record` / `zoom_records` / `delete_zoom_record`, with in-memory `Store`
   tests. The worker gets the records through the same trait seam, so its tests do not need SQLite.
6. CLI `tm alacritty-zoom` (D10), `tm status` line (D11).
7. Keep each new file ≤ ~300 lines; split if it grows.

### P2 — daemon wiring + docs (after P1 is integrated)

1. D8 wiring: run_loop spawn + session-end revert, session transition, `config_apply` plumbing +
   executor.
2. `scripts/uninstall-daemon.sh` (D11).
3. Docs: `CLAUDE.md` gets an architecture entry for `src/alacritty_zoom/`, the `cargo run -- alacritty-zoom` line
   in «Команды», and the keys in «Конфиг». This task doc's status gets updated.

## Tests (inline `#[cfg(test)]`, as everywhere in the repo)

- `plan`: every row of the D6 table.
- `zoom_intent`: `Walking`→`Some(true)`, `Paused`→`Some(false)`, `AwayWhileRunning`→`None`, `Unknown`→`None`.
- `parse_socket_pid`:
  - `Alacritty-48251.sock` → `Some`;
  - `Alacritty-.sock`, `Alacritty-x.sock`, `Alacritty-1.log`, `foo.sock` → `None`.
- `parse_font_size`: real `get-config` JSON excerpt `{"font":{"size":14.0,…},…}` → `14.0`; missing
  `font` / non-number / invalid JSON → `None`.
- `format_font_size`: `14.625`→`"14.625"`, `15.0`→`"15"`, `14.62500001`→`"14.625"`.
- Loader: absent / valid / invalid bool / non-number / `0` / negative / `> 8` / NaN.
- `config_apply` (P2): `diff` detects the field; `apply_config` emits `AlacrittyZoomChanged`.
- Worker with a fake `AlacrittyIpc` (`#[tokio::test(start_paused = true)]`):
  - startup `Revert` touches only recorded pids and never sends anything for unrecorded ones;
  - active → per instance `get-config → record → set → read-back`, and inactive → `get-config → set base → read-back → delete record`;
  - **lossy IPC:** the fake drops the first N sets silently (exit 0, size unchanged), fails with a non-zero exit,
    returns empty `get-config` stdout → the op still converges within `IPC_MAX_ATTEMPTS`. With N ≥ max
    it gives up with one WARN, and the worker keeps running;
  - re-apply with a new delta while zoomed keeps the recorded base (no compounding);
  - `Revert` skips (and deletes the record) when the current size is not ≈ the recorded target;
  - the runner is **never** called with `--reset` (assert on the fake's argv log);
  - active/inactive flapping coalesces to the final state;
  - rescan while zoomed applies only to a new identity, and nothing is rescanned while `Base`;
  - **Alacritty restart while zoomed:** identity A disappears and B appears (same or different pid, other
    start time) → `Apply` on B within one rescan, A's record pruned, no WARN. Then going inactive reverts
    B only;
  - the same pid with a different start time is treated as a new instance (record not reused, no revert of a
    base it never had);
  - an instance that vanishes mid-op → DEBUG, no WARN, not added to `failed`;
  - orphan sockets do not produce per-rescan logs;
  - a failing pid is not retried until the next op;
  - a timing-out call → WARN, the worker keeps running.
- `ipc.rs` runner against a **fake `alacritty` shell script** in a temp dir (never the real Alacritty,
  never the real `$TMPDIR` sockets):
  - it records argv, prints JSON for `get-config`, and asserts `-s <socket>` and `-w -1` placement and that `--reset` never appears;
  - `ALACRITTY_SOCKET`/`ALACRITTY_WINDOW_ID` are absent in the child even if set in the parent;
  - a sleeping variant gets killed by the timeout.
- `discover_instances` on a temp dir with the socket names (use the test's own pid for a "live"
  entry with a basename mismatch → skipped, and a certainly-dead pid → skipped). The real
  `alacritty` basename path is covered by live smoke only.

## Gates (must pass in this order, same as CI)

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo build
cargo test
```

Formatting fixes go in a separate commit. The sandbox has no Alacritty. Nothing in tests may spawn
it or touch the real `$TMPDIR`.

## Live smoke (orchestrator session, operator present)

0. Operator presses **⌘0** in every Alacritty window (clears the manual zoom, constraint 2).
1. `tm alacritty-zoom` → shows the live pid, `base 14 pt → walking 14.625 pt`.
2. `tm alacritty-zoom preview` → font grows like 2×⌘=; `alacritty msg -s <sock> get-config -w -1`
   shows `14.625`. `tm alacritty-zoom reset` → back, `14.0`. A runtime override set beforehand
   (e.g. `window.opacity=0.9`) survives both directions.
3. Guard check: ⌘= once → `preview` → no visible change (expected) → `reset` → ⌘0.
4. `scripts/install-daemon.sh` (mandatory after any rebuild, or toasts silently stop), then `tm alacritty-zoom on`.
5. Walk → grows within ~2 s of `presence transition next_state=Walking` in
   `~/Library/Logs/treadmill-bluetooth-macos/daemon.log`. Then check each way the walk ends:
   - step off with the belt running → **stays big**, including after `AwayWhileRunning`;
   - wait for auto-pause → shrinks;
   - walk again, pause from the remote → shrinks;
   - walk again, `tm stop` → shrinks;
   - walk again, power off the treadmill → shrinks on the session end.
   
   Repeat a few cycles: every transition must land (IPC retries, constraint 3). The log shows at most
   DEBUG retries, no WARN.
6. Mid-walk `launchctl kickstart -k "gui/$(id -u)/com.korniychuk.treadmill-bluetooth-macos.daemon"` →
   revert, then re-apply on `Walking`.
6a. Alacritty restart (D13), the way the operator usually does it:
   - mid-walk → the new window is zoomed within ≤2 s (note whether it opened already zoomed);
   - stop the belt → it shrinks, and `tm alacritty-zoom` shows no stale record;
   - restart while not walking → nothing happens;
   - no WARN in the daemon log for any restart.
7. Mid-walk `tm alacritty-zoom off` → shrinks within ≤5 s (hot reload) or at once (CLI reset).
8. The log has no WARN lines about the orphan sockets, and no `WARN` repeated per tick.
9. The daemon itself proves the launchd context (bare `PATH`, no `ALACRITTY_*` env). The operator skipped
   the manual `env -i` / launchd plist checks on purpose; they are covered here.

## `ankor-dotfiles` follow-up (after smoke, orchestrator session)

- `treadmill/config.toml` receives `alacritty_zoom = true` (and `alacritty_zoom_pt` if calibrated)
  through the `tm` symlink. Commit it there with a pathspec.
- `.alacritty.toml`:
  - a comment at `[font] size = 14` saying the treadmill daemon adds `alacritty_zoom_pt` at runtime while walking,
    ⌘=/⌘- blocks that until ⌘0, and the base is read from this file;
  - an explicit `[general] ipc_socket = true` with a comment ("required by treadmill alacritty-zoom"),
    so a future cleanup does not silently disable it.
- Nothing for Ghostty, herdr or herdr-bar (the font change resizes the pty exactly like ⌘= does).

## Risks / known limitations

- **Reflow on pause/resume:** every shrink/grow resizes the grid (herdr/Claude Code redraw). Stepping off
  no longer triggers it; only a belt stop does.
- **Lossy upstream IPC** (constraint 3) is handled by verify-and-retry. Worth an upstream Alacritty issue
  (the accepted stream should be set back to blocking on macOS). Optional follow-up, not part of this task.
- **Manual zoom wins** (constraint 2), and `tm` cannot detect it (not observable through IPC).
- **A `font.size` pin stays in the running Alacritty process** after the first walk (D4 a). Editing the font size
  in `.alacritty.toml` needs an Alacritty restart to show. That is the price of not using `--reset`.
- **Alacritty upgraded in place while running:** the client binary is newer than the process, so the wire
  format could differ. The read-back WARN catches a silent failure.
- **Mac sleeps mid-walk:** the zoom stays until wake → link loss → revert.

## Non-goals

- Other terminals; detecting which terminal is focused; per-window targeting.
- Reading the manual ⌘= zoom level (impossible).
- A SIGTERM graceful-shutdown handler in the daemon.
- A `daemon_status` snapshot of the zoom state.
