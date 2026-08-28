# 060 — Hygiene: `tm … | head` Broken-pipe panic + split `src/goals.rs`

> **Статус: planned** (2026-08-28)
> **Класс:** hygiene · **Приоритет:** low, two independent items — can be split into two runs.
> **Источник:** observed during 058/059 (`tm status | head` panicked; `goals.rs` reached 985 LOC 🟠 after `led_on_connect`).

## 1. Broken pipe on stdout

**Symptom:** `tm status | head -2` →
`thread 'main' panicked at library/std/src/io/stdio.rs: failed printing to stdout: Broken pipe (os error 32)`.
`println!` panics once the reader closes the pipe. Every read command that
prints multiple lines is affected (`status`, `stats`, `doctor`, `zone list`, …
— 8 files under `src/commands/` + `src/widget.rs` use `println!`).

**Fix (pick one, prefer A):**
- **A.** At the top of `main()` reset `SIGPIPE` to default so the process
  exits quietly like a normal Unix CLI: `unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) }`
  (needs `libc` — check whether it is already a transitive dep; it is a
  one-liner and the standard Rust CLI answer). Must run **before** any
  output. Exclude the `daemon` subcommand? Not needed — daemon writes to a
  log file, SIGPIPE cannot hit it; but keep it uniform.
- **B.** Replace `println!` with a `writeln!(stdout.lock(), …)` that maps
  `ErrorKind::BrokenPipe` → silent `exit(0)`. More churn, no real gain.

**Verify:** `tm status | head -1`, `tm stats --all | head -1`, `tm zone list | head -1`
exit 0 with no panic text on stderr; `tm widget` (TSV, single line) unchanged.

## 2. Split `src/goals.rs` (985 LOC, 🟠)

`goals.rs` has become the whole config-file layer, not just step goals:

| Lines | Concern | Target |
|---|---|---|
| 36–170 | path resolution, symlinks, mtime, `config_path`, `read_config_value` | `src/config_file.rs` (or `src/config/mod.rs`) |
| 118–150, 476–515 | `Goal`, `load_goals`, `assign_tiers`, `thresholds_to_celebrate` | stays `src/goals.rs` |
| 207–260 | `workout_gap_minutes` | `src/config/workout_gap.rs` |
| 264–320 | `auto_pause_minutes` | `src/config/auto_pause.rs` |
| 322–360 | `show_speed` | `src/config/show_speed.rs` |
| 362–447 | `led_on_connect` | `src/config/led_on_connect.rs` |
| 449–475 | `upsert_top_level_key` | `src/config_file.rs` (writer next to reader) |
| 517– | tests | move with their functions |

Rules: pure move, no behaviour change; keep public paths working via
re-exports **or** update every call site (`grep -rn "goals::"` — daemon,
commands, config_apply, widget, zone_hold). The absent-quiet / invalid-WARN
convention and the shared `read_config_value` must stay single-sourced.
Update `CLAUDE.md` module list (it documents `goals.rs` in three places:
main entry, «доп., задача 029», config section) and `docs/tasks/047`'s
mention of the shared reader.

**Verify:** `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test`
(235 tests, same count), `wc -l` of every new file ≤ 300, `tm status` /
`tm led default` / `tm speed-widget` / `tm zone` output byte-identical
before/after (diff the captured output).

## Non-goals

- Changing config semantics or the TOML format.
- Touching `zone_hold/config.rs` (already split in 055).
