# Docs

Documentation-first workspace for `treadmill-bluetooth-macos`.

- `adr/` — Architecture Decision Records.
- `research/` — protocol reverse-engineering notes, BLE captures, findings.
  - [003](research/003-reliability-architecture-review.md) / [004](research/004-independent-reliability-review.md) — reliability plan; tasks **035–047 done**.
- `tasks/` — task specs (`000-name.md`); write before starting work.
  - Reliability `035`–`047` + live smoke [048](tasks/048-live-smoke-035-047.md).
  - Architecture wave `049`–`056`: module splits (store/CLI/daemon/zone_hold),
    scan auto-recover, typed config apply, session state extract, `CentiKmh`.
  - [057](tasks/057-cyan-configurable-values.md) cyan knobs; [058](tasks/058-led-strip-control.md)
    LED strip `tm led on|off`; [059](tasks/059-led-default-on-connect.md) strip
    off by default on connect (**done**, live-verified); [060](tasks/060-hygiene-broken-pipe-goals-split.md)
    hygiene: SIGPIPE panic on piped output + `goals.rs` split (**done**).
- `backlog/` — not-yet-scheduled work. `004`–`011` done (see each file).
- `ideas/` — loose ideas / future directions.
