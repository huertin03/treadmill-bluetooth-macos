# 058 — LED strip control: `tm led on|off`

**Status:** done (2026-08-28). Closes backlog [004](../backlog/004-led-control-via-hci-capture.md).
**Protocol:** [research 007](../research/007-yesoul-led-strip-protocol-apk.md) — decompiled from the official app.

## Goal

Toggle the W2 Pro ambient LED strip from the CLI, through the daemon's live
link (same path as `tm speed`/`start`/`stop`, задача 013), with a direct-BLE
fallback when the daemon does not hold the link.

## Wire contract (from 007)

Write to service `0xFFF0`, characteristic `0xFFF2` (write, no response
expected): `F0 10 02` = on, `F0 10 01` = off. Never write `0xFF00`/`0xFF01`.

## Plan

1. `src/led.rs` (new, pure): `LedState { On, Off }`, `led_frame(LedState) -> [u8; 3]`,
   constants `LED_SERVICE = 0xFFF0`, `LED_WRITE_CHAR = 0xFFF2`, unit tests
   on the bytes. Keep it separate from `fitshow.rs` — this is not a FitShow
   frame (no `02 … xor 03` envelope).
2. `control_command.rs`: add `ControlCommand::Led(LedState)`, text wire form
   `led:on` / `led:off` (queue table unchanged).
3. `daemon/commands.rs`: execute `Led` with the same bounded write helper
   used for Control Point writes (задача 012's timeout), `WARN` on failure
   (edge case only), no retry loop beyond what the write helper does — the
   app's 3× retry is for flaky Android stacks.
4. `control.rs` (direct path): `set_led(peripheral, state)` for the
   no-daemon fallback, mirroring how `set_speed` is dispatched from the CLI.
5. CLI `tm led on|off` (no arg → print last requested state if we persist
   it; otherwise usage). Persist nothing in config for v1 — the treadmill
   remembers its own state; the app only persists a preference to re-send
   on connect. **Decide at impl time** whether to re-send on daemon
   reconnect (would need a config key `led = on|off`; YAGNI unless the
   treadmill forgets the state on power-cycle — check live).
6. Docs: `CLAUDE.md` (module list + command list), `docs/README.md`,
   backlog 004 → done.

## Verification

- `cargo test` for `led_frame`.
- Live: phone app **disconnected** (single-central device — the daemon must
  own the link), `tm led off` → strip goes dark, `tm led on` → strip lights;
  check `RUST_LOG=debug` shows the write ACK and no Control-Point
  side-effects (speed unchanged).
- Also observe whether `F0 10 00` (sent by the app on every connect) is
  needed for anything — we do **not** send it in v1.

## Non-goals

- Strip colour via `d18d2c10` extension frames (documented in 007; separate
  idea).
- Incline (RF-only, задача 003 verdict).
