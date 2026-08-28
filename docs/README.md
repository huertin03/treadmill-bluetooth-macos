# Docs

Documentation-first workspace for `treadmill-bluetooth-macos`.

- `adr/` — Architecture Decision Records.
- `research/` — protocol reverse-engineering notes, BLE captures, findings.
  - [003](research/003-reliability-architecture-review.md) / [004](research/004-independent-reliability-review.md) — reliability plan; tasks **035–047 done**.
- `tasks/` — task specs (`000-name.md`); write before starting work.
  - Reliability `035`–`047` + live smoke [048](tasks/048-live-smoke-035-047.md).
  - Architecture wave `049`–`056`: module splits (store/CLI/daemon/zone_hold),
    scan auto-recover, typed config apply, session state extract, `CentiKmh`.
- `backlog/` — not-yet-scheduled work. `005`–`011` done (see each file);
  open: [004](backlog/004-led-control-via-hci-capture.md) (hardware-blocked,
  unblocked 2026-08-28 by [research 007](research/007-yesoul-led-strip-protocol-apk.md) — LED frame
  from APK decompile; impl = task [058](tasks/058-led-strip-control.md), planned).
- `ideas/` — loose ideas / future directions.
