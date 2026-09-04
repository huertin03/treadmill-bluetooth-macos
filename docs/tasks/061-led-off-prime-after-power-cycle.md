# 061 — LED `off` is a no-op after a mains power-cycle: prime with `on` first

> **Статус: done** (2026-09-04), pending live verification on the next power-cycle.
> **Класс:** bugfix · **Приоритет:** low (cosmetic strip), follows [058](058-led-strip-control.md) / [059](059-led-default-on-connect.md).
> **Источник:** operator 2026-09-04 — `led_on_connect = "off"` did not darken the strip after the treadmill re-lit it.

## Observation (2026-09-04 11:56–11:57, daemon log)

Six queued commands, all `RequestControl` `0x00` ack'd, every `0xFFF2` write
ATT-ACK'd, no WARN:

| id | command | strip | console |
|---|---|---|---|
| 1–4 | `led:off` ×4 | stays **on** | silent |
| 5 | `led:on` | stays on | **beep** |
| 6 | `led:off` | **dark** | — |

The on-connect write (`applied led_on_connect … state=off` at 10:58 and 11:44)
had the same silent no-op result.

## Diagnosis

Transport is fine (058 live-verified the same frames on 2026-08-28). The
firmware keeps its own LED state variable and only acts on a **transition**:
after a mains power-cycle the variable resets to *off* while the hardware
default re-lights the strip. `F0 10 01` (off) on a firmware that already
believes *off* does nothing; `F0 10 02` (on) flips the variable (beep, no
visible change); the next `F0 10 01` is a real off→on→off transition and
works. The app never hits this because it sends `F0 10 00` ("light reset",
unverified semantics) before its persisted preference — we deliberately do
not send `F0 10 00` (058).

## Fix

`Controller::set_led(Off)` always primes with `On`, waits
`LED_PRIME_DELAY` (100 ms — the app's own retry spacing), then writes `Off`.
`On` is unchanged. Applies to both paths (`led_on_connect` and `tm led off`),
since the CLI path showed the identical failure. Cost: one console beep and
a ≤100 ms flash when the strip was already dark. Strip state is not
readable on this firmware (059), so there is no way to skip the prime when
it is unnecessary.

## Out of scope / follow-up

- Probing `F0 10 00` as a cheaper sync primitive (backlog candidate; the app
  fires it on every connect, so it is at least safe on this firmware).

## Verification

- `cargo test` (frame constants unchanged; prime order asserted by the
  existing `set_led` write sequence — no BLE mock in the crate, so the
  order is by inspection).
- Live: mains power-cycle → strip re-lights → daemon connects → beep →
  strip dark. Then `tm led on` / `tm led off` still toggle as in 058.
