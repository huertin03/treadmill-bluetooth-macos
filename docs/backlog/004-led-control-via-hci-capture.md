# 004 — LED backlight control (vendor command via HCI capture)

**Status:** backlog (deferred by operator, 2026-07-05; re-surveyed 2026-08-28 — see
[research 006](../research/006-led-control-2026-resurvey.md): still no public frame)
**Depends on:** [003](../tasks/003-yesoul-w2-pro-controller.md) findings.

## Context

The official Yesoul app can toggle the W2 Pro's LED backlight over BLE, so a
vendor command exists — but it is documented nowhere public (not in the
FitShow protocol, treadspan, or qdomyos-zwift sources). The write target is
almost certainly one of: `d18d2c10-c44c-11e8-a355-529269fb1459` (write, inside
FTMS), `0xFAB1`/`0xFAB2`, or `0xFFF2`.

Note: the app exposes NO incline control (verified by operator, including
in-app workouts) — incline stays RF-remote-only; this task is LED only.

## Plan (when picked up)

1. Operator's Android (Xiaomi) phone: enable Developer options → **Bluetooth
   HCI snoop log** + USB debugging; toggle Bluetooth off/on to start a fresh log.
2. In the Yesoul app, connect to the treadmill and toggle the LED ~5 times
   with ~2 s pauses.
3. Pull the log over USB (`adb bugreport` → btsnoop_hci.log) and analyze in
   tshark, filtering ATT writes (same methodology as the Sperax pcap analysis).
4. Identify the LED frame(s) + target characteristic; implement `led on|off`
   in the CLI (likely via `src/fitshow.rs` or a new vendor module); verify live.

⚠️ Never write to `0xFF00`/`0xFF01` (suspected OTA channel) — firmware changes
are forbidden without explicit operator approval.

## 2026-08-28 refresh (research 006)

Internet re-survey found **no** open-source LED frame for Yesoul / Sperax /
FitShow boards; FitShow v1.1 and Fit Monster 2026 opcode tables have no light
command; `d18d2c10-…` is an OEM v1 UUID with no public doc (only time-sync seen
on Sperax RM-01). Capture is still the only way. Amended plan, ranked:

1. **jadx the China APK `yesoul.yesoulmobile`** (intl: `com.yesoulchina.international.bicycle`):
   `rg '氛围灯|灯光|灯带|backlight|ledStrip|setLight|d18d2c10|FAB1|fff2'`.
   No treadmill needed; yields candidate `byte[]`, confirm live afterwards.
2. **Xiaomi/HyperOS HCI snoop** — Developer options toggle alone is often not
   enough: dial `*#*#5959#*#*` to arm logging, BT off/on, toggle LED ≥5×
   (+1 speed change as positive control), dial `5959` again to dump, pull from
   `/sdcard/MIUI/debug_log/common/` (or `adb bugreport out.zip` →
   `FS/data/misc/bluetooth/logs/btsnoop_hci.log`). Android 16 has no live
   `androiddump`; file-based only. Wireshark: `btatt.opcode in {0x12,0x52}`.
3. Fallback: Frida hook on `BluetoothGatt.writeCharacteristic` (in-process,
   survives HyperOS snoop bugs).
4. Fallback: nRF52840 sniffer / iOS PacketLogger / Bumble MITM.

Constraint (operator, 2026-08-28): while the phone app is connected the
treadmill drops the Mac link (single-central device), so the Mac cannot
co-observe — capture must happen on the phone side (or OTA sniffer).
