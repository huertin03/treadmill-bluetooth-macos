# 059 — LED strip default state on connect (`led_on_connect`)

**Status:** done, pending live verification (2026-08-28). Follows [058](058-led-strip-control.md).

## Context

The W2 Pro turns its ambient LED strip back **on** every time it is
re-powered from mains. The official app compensates by persisting a per-device
preference and re-sending the LED command on every connect
(`UserDataManager.saveIsOpenLight` + `writeCmdLight`, research
[007](../research/007-yesoul-led-strip-protocol-apk.md)). There is **no
read-back**: `0xFFF2` is write-only and nothing reports the strip state, so
the only workable semantics is fire-and-forget after connect.

Operator wants: strip **off by default** whenever the daemon connects.

## Design

- New optional **top-level** `config.toml` key `led_on_connect = "off" | "on"`
  (absent → do nothing, i.e. today's behaviour). Same absent-quiet /
  invalid-WARN style as `show_speed` (`goals::load_show_speed`,
  `src/goals.rs:335`); parse via `led::LedState::from_str`.
- `LiveConfig`/`ConfigDelta` (`src/config_apply.rs:16-37`, `diff` :99,
  `reload_if_changed` :124, `apply_config` :145) gain `led_on_connect:
  Option<LedState>` so hot-reload picks it up (задача 017 pattern; the value
  only matters at the next connect — no immediate effect on reload, and that
  is deliberate: reload must not blink the strip).
- Daemon: once per treadmill session, right after `scan::connect_treadmill`
  succeeds and before/at the start of `stream_with_presence`
  (`src/daemon/run_loop.rs:317-326`, `src/daemon/session.rs:57`), if the key
  is set → `Controller::take_control` + `set_led(state)` inside the same
  bounded timeout used by `restore_speed` (`src/daemon/speed.rs:91`,
  `SPEED_RESTORE_TIMEOUT`). Failure = `WARN` and continue (strip is
  cosmetic; never block telemetry). Put the helper in a small
  `src/daemon/led.rs` mirroring `daemon/speed.rs`, not inline in `session.rs`.
- Reconnect after a BLE drop counts as a connect (idempotent write; the
  treadmill keeps state across BLE drops, so it is a no-op there).
- CLI setter: `tm led default off|on|none` (no arg → prints current value,
  cyan per задача 057), implemented with `goals::upsert_top_level_key`
  (`src/goals.rs:362`) like `tm speed-widget`; `none` removes/comments the
  key — check what `upsert_top_level_key` supports before promising removal;
  if it cannot delete, write `led_on_connect = "none"` and treat `"none"` as
  absent in the loader.
- `tm status`: one line `led on connect: off` (cyan) in the config block
  (задача 022 snapshot; store the loaded value in `daemon_status` only if
  the existing snapshot columns pattern makes it cheap — otherwise read-time
  from the file is fine, mirror `workout_gap_minutes`).
- `config/config.example.toml`: commented `# led_on_connect = "off"` with
  the mains-power rationale.

## Verification

- Unit: loader (absent/valid/invalid), `ConfigDelta` diff for the new field,
  `led_on_connect` decision helper (pure: config value → `Option<LedState>`).
- Live: set `led_on_connect = "off"`, power-cycle the treadmill at the mains
  (strip comes on by itself) → daemon connects → strip goes dark without any
  CLI call. Then `tm led on` still works manually.

## Non-goals

- Reading the strip state back (impossible on this firmware).
- Sending `F0 10 00`.

## Live verification log (2026-08-28)

- `tm led default off` → config `led_on_connect = "off"`; daemon hot-reload logged
  `loaded config (… + led_on_connect)`.
- Strip manually **on**, then `launchctl kickstart -k` → daemon reconnected 21:22:16 and
  logged `applied led_on_connect after treadmill connect state=off` — reconnect path verified.
- Mains power-cycle path (strip re-lights by itself → daemon connects → dark) — same code
  path; to be observed by the operator on the next power-cycle.
