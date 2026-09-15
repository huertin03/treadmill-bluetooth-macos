# 063 — Keyboard belt control: `tm toggle` + `tm speed up|down` (UHK + Karabiner)

> **Статус: done** (2026-09-15). Rust side implemented; Karabiner side is in `macos-keyboard`.
> **Класс:** feature · **Приоритет:** medium. Builds on [013](013-control-commands-via-daemon-queue.md) (daemon control queue), [039](039-control-source-and-operator-override.md) (control source / Zone Hold override window), [054](054-speed-centi-newtype.md) (`CentiKmh`).
> **Источник:** operator 2026-09-15 — control the belt from the external UHK keyboard: Cmd+Play/Pause = start/stop toggle, Cmd+Previous/Next = slower/faster. Only these three actions.

## Goal

Three key chords on the UHK (never the MacBook built-in keyboard) drive the belt
through the existing daemon queue:

| Chord (UHK only) | Shell command | Effect |
|---|---|---|
| Cmd + Play/Pause | `$HOME/.bin/tm toggle` | belt moving → Stop; stopped → Start |
| Cmd + Rewind (prev) | `$HOME/.bin/tm speed down` | target −0.1 km/h |
| Cmd + Fast Forward (next) | `$HOME/.bin/tm speed up` | target +0.1 km/h |

No sockets, no new IPC: Karabiner `shell_command` → `tm` CLI → SQLite
`control_commands` queue → daemon executes on the live BLE link (задача 013).

## Why relative commands resolve in the daemon, not in the CLI

A CLI-side `speed up` (read `daemon_status.last_speed_kmh`, enqueue an absolute
`speed:X`) loses presses: three quick presses spawn three `tm` processes that all
read the same stale speed → +0.1 instead of +0.3. The belt also ramps, so live
telemetry lags the commanded target for a second or more. Only the daemon sees
both the live speed and what it has just commanded, so `speed_step:*` and
`toggle` are queued as **relative intents** and resolved at execution time.

## Decisions

1. **Step = 0.1 km/h**, a constant (`SPEED_STEP = CentiKmh(10)`). Device range is
   0.50–6.10 km/h with increment 0.1 (research 001, Supported Speed Range
   `0x2AD4`). No config key (YAGNI). Result is clamped to
   `[SPEED_MIN = CentiKmh(50), SPEED_MAX = CentiKmh(610)]`.
2. **Queue wire forms** (text column unchanged, no schema change):
   `speed_step:up`, `speed_step:down`, `toggle`. Existing `start` / `stop` /
   `speed:<kmh>` / `led:*` stay byte-identical.
3. **Intent memory, window `INTENT_WINDOW = 5 s`.** A pure struct (no BLE, time
   injected as `Instant`) remembers the last *target speed* the daemon wrote and
   the last *start/stop* it issued, each with a timestamp.
   - **Speed step base:** last target if written < 5 s ago, else live telemetry
     speed. Every successful daemon speed write records the target — CLI
     `speed:`/`speed_step:`, resume restore, default speed, Zone Hold — so a
     press during a restore ramp steps from the restore target, not from the
     ramping belt.
   - **Speed step refused** (row marked `failed`, WARN, no BLE write) when the
     base is unknown, the belt is stopped (base == 0), or a Stop was issued
     < 5 s ago (belt decelerating). A speed key must never set a moving target
     on a stopping or stopped belt. Only `toggle` may start the belt.
   - **Toggle:** a Start/Stop issued < 5 s ago → the opposite of it (cancel a
     start during the console countdown, when telemetry still reads 0). Else
     live speed > 0 → Stop; live speed == 0 → Start; live speed unknown →
     **Stop** (safe side) + WARN.
   - Only CLI-issued Start/Stop/toggle record the run intent; auto-pause's Stop
     does not (the operator is away by definition).
4. **Stop opcode:** reuse the existing `Controller::stop()` / `start()`. The
   resume path (задача 012) already restores the pre-pause speed after Start.
5. **Zone Hold:** a resolved `speed_step` counts as a CLI speed write and opens
   the operator override window exactly like `speed:` (`zone.note_cli_speed`).
   Toggle does not.
6. **Direct-BLE fallback** (daemon not holding the link): `toggle` and
   `speed up|down` bail with a clear message — they need live telemetry and the
   daemon's intent memory. Absolute `tm speed <kmh>` / `start` / `stop` keep
   their fallback.
7. **CLI surface:** `tm speed <kmh|up|down>` (one positional, parsed by a small
   `FromStr` type; `tm speed 3.2` unchanged) and a new `tm toggle`. Success
   lines: `speed stepped up` / `speed stepped down` / `belt toggled`. The
   resolved value is logged by the daemon, not printed by the CLI.
8. **No toast / no new widget field.** The console and the existing speed widget
   (задача 029) show the result.

## Karabiner side (repo `macos-keyboard`, done by the orchestrator)

- Goku 0.8.0 (also upstream master) cannot emit `consumer_key_code`
  `scan_next_track` / `scan_previous_track` — the codes the UHK sends today for
  next/prev (HID 181/182, current UHK Agent backup). It *can* emit `fast_forward`
  / `rewind` (179/180) and `play_or_pause` (205).
- **Manual step (operator, UHK Agent):** remap the two track keys from
  *Next Track / Previous Track* to *Fast Forward / Rewind* on every layer where
  they live. macOS treats them as next/previous track (it is what Apple
  keyboards' F9/F7 send), so plain audio control is unchanged.
- Rules gated by `[:uhk]`, mandatory `command` (either side), block placed
  after "UHK: Disable cmd+tab":
  `{:ckey :play_or_pause :modi :command}` → `"$HOME/.bin/tm toggle"`, etc.
- `shell_command` is confirmed working on KE 16.1.0 (Karabiner log 2026-09-09
  shows `open -a` stderr), despite the stale July note in `karabiner.edn`.
- `~/.bin/tm` → release binary; `scripts/install-daemon.sh` refreshes it.

## Plan (Rust side — executor)

1. `src/control_command.rs`: add `SpeedStep(StepDirection)` and `Toggle`
   variants, wire forms above, round-trip + garbage tests.
2. New pure module `src/belt_intent.rs` (name free to adjust): the intent memory
   of decision 3 with `note_speed`, `note_run`, `resolve_step`, `resolve_toggle`;
   constants `SPEED_STEP`, `SPEED_MIN`, `SPEED_MAX`, `INTENT_WINDOW`. Unit tests
   for every branch listed in decision 3 (fresh vs expired target, clamp at both
   bounds, refused when stopped / unknown / recent Stop, toggle with recent
   Start, recent Stop, moving, stopped, unknown).
3. Live speed as `Option<CentiKmh>`: keep the last decoded `data.speed` next to
   the existing `state.last_speed_kmh` update in `src/daemon/session.rs`
   (`TreadmillLink` is the natural owner); reset on link loss like the snapshot.
4. `src/daemon/commands.rs::process_control_commands`: take the intent + live
   speed, resolve relative commands into concrete `Start`/`Stop`/`Speed` before
   `execute_control_command`; a refused step → `mark_control_command_failed`
   with a human reason + WARN (`command`, `live_speed`, reason). Log the
   resolution on the existing success line (`command=speed_step:up
   resolved=speed:3.3`). Record intent only after a successful write. Return
   `true` for a resolved speed so Zone Hold's override window opens.
   `execute_control_command` must never receive an unresolved variant (bail if
   it does).
5. Record target speed on the other daemon speed writes (`src/daemon/speed.rs`
   restore + default speed, `src/daemon/zone_write.rs`).
6. CLI: `src/main.rs` `Commands::Speed` positional → `SpeedTarget`
   (`Absolute(CentiKmh) | Up | Down`), new `Commands::Toggle`;
   `src/commands/belt.rs` direct-BLE fallback bails for the relative variants;
   `describe_control_success` covers them.
7. Docs: `CLAUDE.md` (module list + Команды), this file's status.

## Gates

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo build
cargo test
```

Formatting fixes as a separate commit. No BLE in tests. Do not touch firmware
or vendor channels.

## Verification (operator, live)

1. Rebuild + `scripts/install-daemon.sh`.
2. Belt stopped: `tm speed up` → fails with "belt is stopped", nothing moves.
3. On the belt: Cmd+Play → starts; Cmd+Next ×3 quickly → +0.3; Cmd+Prev → −0.1;
   Cmd+Play → stops; Cmd+Play within 5 s of a Start → stops.
4. MacBook built-in keyboard: Cmd+media keys do nothing treadmill-related.
