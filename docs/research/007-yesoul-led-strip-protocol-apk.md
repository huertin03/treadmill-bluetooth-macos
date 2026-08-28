# 007 — Yesoul W2 Pro LED strip protocol (from APK decompile)

**Status:** research, 2026-08-28. Source: official Yesoul app
`com.yesoulchina.international.bicycle` pulled from the operator's Xiaomi
(Android 16 / HyperOS 3.0) via `adb pull`, decompiled with `jadx 1.5.6`.
No HCI capture was needed. Supersedes the "unknown vendor write" guesses in
[001](001-yesoul-ble-protocol.md) §Phase 3 and [006](006-led-control-2026-resurvey.md).
Feeds task [058](../tasks/058-led-strip-control.md); unblocks backlog
[004](../backlog/004-led-control-via-hci-capture.md).

## The LED toggle (confirmed in code, not yet on hardware)

`YesoulBleUtils.writeCmdLight(name, isOpen, …)` (also duplicated in
`SearchTreadmillActivity.writeCmdLight`):

| Action | Service | Characteristic | Bytes |
|---|---|---|---|
| LED strip **on**  | `0xFFF0` | `0xFFF2` (write) | `F0 10 02` |
| LED strip **off** | `0xFFF0` | `0xFFF2` (write) | `F0 10 01` |
| "light reset" sent on every connect (`SmartKnobUtils.rightCmdLight`) | `0xFFF0` | `0xFFF2` | `F0 10 00` |

- The app retries the write up to 3× with 100 ms delay on `onWriteFailure`.
- No response is expected on any notify char — consistent with the 2026-07
  probes: `FFF2` writes are ATT-ACKed and never answered (those probes were
  FitShow `02 … xor 03` frames, which this firmware ignores).
- Chinese log string: `氛围灯` = "ambient light".
- The app treats it as a **per-device persisted preference**
  (`UserDataManager.saveIsOpenLight(deviceName, bool)`) and re-sends it after
  connect; semantics of `F0 10 00` (auto/default mode?) are unverified — the
  app fires it right before subscribing to notifications, on every connect.

Other `FFF2` frames seen in the app (not needed, listed for the record):
`F0 F0 02 01` (`writeBlueControl` — "bluetooth control" handshake during
pairing/OTA search; NOT sent in the normal training flow), `F5 20 20 40 F6`
(bike handshake, `0xFFF1` path), `F5 F1 24 01 BA F6` etc. (SmartKnob bike
console UI). ⚠️ Do not replay these on the treadmill.

## Which devices get the toggle in the UI

`UserPairDeviceDetailActivity`: the switch is shown if the BLE name starts with
`YS_W2PRO_` or `YS_KT30`, **or** feature bit `supportLightStrip` is set.

## `0xFF00`/`0xFF01` is a *feature bitmap*, not OTA

`YesoulBleUtils.readYesoulMachineFeatureBean` **reads** `0xFF01` (service
`0xFF00`), 4 bytes LE → `YesoulMachineFeatureBean`:

| bit | flag |
|---|---|
| 0 | `!isBritishSystemSupported` |
| 1 | `supportUnitSwitch` |
| 2 | `supportCalorieDecimalSwitch` |
| 3 | `supportSyncData` |
| 4 | `supportWiFi` |
| 5 | `isTargetResistanceCalcSupported` |
| 6 | **`supportLightStrip`** |
| 7 | `supportLightRingRGB` |
| 8 | `supportFiveColorLightRing` |
| 9 | `supportFaultReport` |
| 10 | `supportOTA` |
| 11 | `supportWiFiAuthScreenCast` |
| 12 | `supportBluetoothAuthScreenCast` |

Our GATT snapshot read `4f 02 00 00` = `0x024F` → bits {0,1,2,3,6,9}:
unit switch, calorie decimal, sync data, **light strip**, fault report.
`supportOTA` = 0. So the 2026-07 "do not write `0xFF00`" rule stays as a
matter of hygiene (we still never *write* there), but reading `0xFF01` is
the app's own capability probe and is safe. OTA in the app goes through a
separate `WSOTA` lib on `0xFAB0`.

## Bonus: FTMS extension data on `d18d2c10-…` (console sync, incl. strip colour)

`SmartKnobUtils.writeExtensionData` writes to `d18d2c10-c44c-11e8-a355-529269fb1459`
(inside FTMS `0x1826`) a frame `[flags u16 LE][fields…]` where each field is
appended only if present and sets its flag bit:

| bit | field | encoding |
|---|---|---|
| 1 | heart rate | u8 |
| 2 | calories | u16 LE |
| 4 | step count | u16 LE |
| 5 | elapsed seconds | u16 LE |
| 6 | **RGB colour** | 3 bytes `RRGGBB` |
| 7 | distance | u24 LE |

The treadmill strategy sends this ~every second while `supportSyncData`;
when `isLightControl` (name `YS_KT30` or `supportLightStrip`) the RGB is
`changeSpeedData(speed)`: ≤2 km/h `146FFF`, ≤4 `00C2AB`, ≤6 `BCDB00`,
≤8 `FBAE00`, ≤10 `FF7F20`, ≤12 `EE276F`, else `905CFF`. For P35 bikes the
colour is by HR-zone instead (`ffffff/009bfe/49ce59/fdc901/fd0001`).
The RM-01 pcap "time-sync" frames on this UUID were this same sync packet.
Possible future feature: colour the strip by our own HR zone. Out of scope
for 058.

## Unit switch (for the record)

`0xFFF2` ← `0f` (metric) / `f0` (imperial); `0xFFF1` read returns the
current value (`f0` in our snapshot → **imperial** flag set on this unit?
unverified — `isImperial()==0 → "0f"` in `writeImperialToDevice`).

## Method

`adb pull` of `base.apk` (+ splits) → `jadx --no-res` → grep for
`d18d2c10|fab|fff2|Light` → follow `isLightControl` / `writeCmdLight`.
Native libs (`libysjni.so`, `libxeno_native.so`) contain no BLE logic.
~30 min wall-clock; HCI snoop on HyperOS 3 would have needed manual toggles
(`adb shell settings put secure bluetooth_hci_log` is denied to shell).
