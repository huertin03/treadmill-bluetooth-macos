# 065 — Explicit start speed

Add optional `tm start --speed KMH`, retaining bare `start` semantics and its
queue wire form. The explicit variant is one queue item (`start-speed:KMH`)
and uses the same implementation for direct BLE and daemon execution.
Explicit starts expire after 5 seconds in the queue, execute within 19 seconds
(including cleanup), and have a 25-second CLI wait budget. Existing commands'
expiry and wait behavior are unchanged. Both CLI and daemon need the update.

Validate finite, positive input and the existing sane ceiling before routing.
Read the device's Supported Speed Range before starting; reject missing,
malformed, out-of-range or off-increment targets without a Start write.
Then request control, start, and set speed in order. Success reports target
acknowledgement, NOT measured belt speed. No automatic retry of Start.
If Start or Set Speed fails or times out after a Start attempt, attempt a
bounded Stop and report both outcomes; never claim a guaranteed physical stop.

Explicit intent consumes the session default and suppresses pre-pause restore
for the next resume within 30 seconds. Zone Hold gets its existing manual
override window; safety Stop remains active. These guards also cover uncertain
partial failure so automation cannot immediately overwrite the operator.

Use offline tests for validation, wire compatibility, operation ordering,
failure/timeout cleanup and resume precedence. Hardware timing/countdown and
physical speed must be reviewed separately before this change is released.
Do not install the development binary or send commands to a live treadmill
as part of these tests. No incline changes.
