# 008 — Alacritty 0.17 IPC and runtime font size

**Date:** 2026-09-15. **Feeds:** task [062](../tasks/062-alacritty-zoom-while-walking.md).
**Sources:** `alacritty/alacritty` tag `v0.17.0` (commit `94e7c88`, the version installed
on the operator's Mac), `crossfont` v0.8.1 (pinned in its `Cargo.lock`), plus live probes
on the host. Line numbers refer to that tag.

## Host facts (2026-09-15)

- `alacritty 0.17.0 (94e7c88)`. Cask app is `/Applications/Alacritty.app/Contents/MacOS/alacritty`,
  and `/opt/homebrew/bin/alacritty` symlinks to it.
- Config is `~/.alacritty.toml`, a symlink to `ankor-dotfiles/.alacritty.toml`. It sets `[font] size = 14`,
  has no `IncreaseFontSize` rebind (so the macOS defaults ⌘= / ⌘- / ⌘0 apply) and does not set
  `ipc_socket` (default `true`).
- One live Alacritty process, plus **13 orphaned** `Alacritty-<pid>.sock` files in `$TMPDIR`
  left by crashes or kills.
- The launchd daemon's environment has the **same `TMPDIR`**
  (`/var/folders/j2/…/T/`) and `PATH=/usr/bin:/bin:/usr/sbin:/sbin` (no Homebrew).
  launchd `exit timeout = 5`.
- Shells inside Alacritty can carry a **stale** `ALACRITTY_SOCKET`. The herdr server inherited
  `Alacritty-1806.sock` from a dead process, while the live one is 48251. Never trust
  that env var, and never trust `ALACRITTY_WINDOW_ID` either.
- `alacritty msg -s <sock> get-config -w -1` prints JSON of the whole config and has
  `"font":{…,"size":14.0}`. Per-window `-w <id>` also printed `14.0` while the window was
  visibly zoomed with ⌘=, which confirms that runtime zoom is invisible to IPC.

## Source facts

1. **Manual zoom blocks config-driven size changes.** `window_context.rs:275-284`:
   ```rust
   // Do not update font size if it has been changed at runtime.
   if self.display.font_size == old_config.font.size().scale(scale_factor) {
       self.display.font_size = self.config.font.size().scale(scale_factor);
   }
   ```
   - If the window was zoomed with ⌘=/⌘-, an IPC `font.size=X` updates the config but **not the
     visible size**. `--reset` behaves the same way.
   - On an un-zoomed window IPC applies immediately, and a later `--reset` shrinks it back.
   - ⌘0 (`reset_font_size`) sets the size to the **merged** config (file + IPC overrides), so it
     "re-syncs" the window and automation works again.
2. **⌘= step is 1 physical px on the scale-factor-scaled size** (`input/mod.rs:56`
   `FONT_SIZE_STEP = 1.`, `event.rs:921` `as_px().round() + delta`). With crossfont
   px = pt·96/72 and `display.font_size = config pt × scale_factor`:
   - Retina (×2), base 14 pt: 28 pt = 37.33 px → 38 → 39 px = 29.25 pt → **14.625 config pt**
     after two presses, i.e. **+0.625 pt**.
   - ×1 display: +1.75 pt. With base 13 @×2 it is +0.875 pt, because the rounding shifts.
   - So "N presses" is not a fixed pt delta. A pt delta in config units gives the same
     *logical* zoom on every display, because Alacritty rescales pt per display itself.
3. **`-w -1` covers new windows.** For `window_id = None` the options are appended to every window
   **and** to `global_ipc_options`, which `create_window` copies into windows opened later
   (0.13.0: "Copy global IPC options (-w -1) for new windows").
4. **Overrides accumulate without bound.** `add_window_config` does `extend_from_slice` into a
   per-window `Vec` (no de-dup by key), and every later `update_config` replays the whole history.
   `--reset -w -1` does `window_config.clear()` on every window plus `global_ipc_options.clear()`,
   so it clears **all** runtime keys, not just `font.size`.
5. **`get-config -w -1`** returns the file config plus `global_ipc_options` only (`event.rs:320-341`),
   serialized as the whole `UiConfig`. It **includes current `-w -1` overrides**, so it cannot tell the
   file base from an active override. A naive "read base, add delta" after a crash compounds.
6. **Sockets.**
   - The path is `env::temp_dir()/Alacritty-<pid>.sock` on macOS (`polling/mod.rs:47-53`,
     `polling/ipc.rs:152-232`), with no display prefix, and it is removed by `Drop` on clean exit only.
   - Without `--socket`, `find_socket` connects to the **first** connectable socket in readdir
     order and deletes orphans on `ConnectionRefused`. With several processes it reaches
     only one of them, chosen arbitrarily.
7. **Errors are mostly silent.**
   - `msg config` gets **no reply** and exits 0 even for an invalid option or an unknown window id.
     Those errors only show in the target window's message bar.
   - Non-zero exit happens only when the socket cannot be found, connected to or written.
   - `get-config` does wait for a reply line, so a hung Alacritty hangs the client.
   - Wire format: `SocketMessage` serde JSON, no version field, not documented as stable.
     Use the CLI, preferably the **same binary as the running process**.

## Consequences for the design

- Mechanism: `alacritty msg -s <socket> config -w -1 …` per live process, spawned with a timeout.
  Do not reimplement the socket protocol.
- Rejected: revert with `--reset -w -1`. It would bound fact 4, but it also clears every other runtime
  override, and the operator wants the font changed only (2026-09-15). Revert instead writes the
  recorded base back: `config -w -1 font.size=<base>`. The base comes from a zoom record persisted
  before applying, because of fact 5. See task 062 D3/D4.
- Always read back with `get-config` after `config` (fact 7).
- Find live processes from the socket file names: pid → `proc_pidpath` → basename `alacritty`.
  Never probe by connecting.
- The operator must stop zooming with ⌘= (fact 1). ⌘0 re-syncs a manually zoomed window.
