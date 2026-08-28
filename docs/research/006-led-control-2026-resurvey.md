# 006 — LED backlight control: 2026 internet re-survey (Grok, read-only)

**Status:** research, 2026-08-28. Feeds backlog [004](../backlog/004-led-control-via-hci-capture.md). Grok run `led-research`, brief in scratchpad; report verbatim below.


# LED backlight control on the Yesoul W2 Pro — 2026 state of the art

**Date:** 2026-08-28  
**Scope:** read-only internet + this repo’s prior hardware notes. No repo changes.  
**Device under discussion:** Yesoul W2 Pro (`YS_W2PRO_02395`, FW `SDC_W2_BT_V3.03-50-54`, manufacturer `YESOUL`). Confirmed GATT: FTMS `0x1826` plus vendor `d18d2c10-…` (write, *inside* FTMS), `0xFAB0`, `0xFFF0`, `0xFF00`. Identical GATT previously seen on Sperax RM-01.

**Bottom line:** still **no public byte frame** for the LED strip. Every open-source treadmill stack that speaks FitShow / Sperax / Yesoul implements speed, start/stop, and (sometimes) incline or vibration — never a light/lamp/atmosphere command. The FitShow published opcode table has no LED slot. The UUID `d18d2c10-c44c-11e8-a355-529269fb1459` has **no public protocol document**; the only documented traffic on it is a **time-sync** write from a Sperax RM-01 pcap (this repo, 2026-07). Getting the LED frame still requires capturing the official app.

---

## 1. Open-source implementations of LED / light / lamp

**Verdict: not found.** No GitHub/GitLab/HA/ESPHome/qdomyos/treadspan/openant project publishes an LED/light/lamp/ambient-light command for Yesoul, Sperax, SPAX, FitShow, or “WalkingPad-like” OEM boards on `d18d2c10-…`, `FAB1`/`FAB2`, or `FFF2`. No byte frames exist to copy.

What *does* exist, and why it does not answer the question:

| Project | What it actually implements | LED? |
|---|---|---|
| [cagnulein/qdomyos-zwift](https://github.com/cagnulein/qdomyos-zwift) FitShow treadmill | Start/stop/speed/incline over FitShow `0x02…xor…0x03` on `0xFFF0`. Settings expose only a FitShow User ID. Repo code search for `led` in that tree: **0 files**. Yesoul support is **bikes** (`0xFFF0`/`0xFFF1`/`0xFFF4` handshake `F5 20 20 40 F6`), plus a generic-FTMS fallback for a device named `"YESOUL"` that lacks `0xFFF0`. | No |
| [mcdax/hass-walkingpad](https://github.com/mcdax/hass-walkingpad) | KingSmith WiLink + FTMS + **Sperax P3 Max** vendor protocol on `0xFFF0` (incline 0–10, vibration 0–4). Explicitly: Sperax **RM-01** is FTMS, same class as W2 Pro. Extra entities: incline + vibration. | No |
| [blak3r/treadspan](https://github.com/blak3r/treadspan) | ESP32 mimics LifeSpan / UREVO / Sperax apps to pull steps. Protocol-analysis folder is LifeSpan-centric. | No |
| [ph4r05/ph4-walkingpad](https://github.com/ph4r05/ph4-walkingpad) | KingSmith WalkingPad: `0xFE01`/`0xFE02`, frames `F7 A2 … FF FD`. Different OEM. | No |
| [tyge68/fitshow-treadmill](https://github.com/tyge68/fitshow-treadmill) (cited by ESPHome F15) | FitShow JS client, speed/status. | No LED in public docs |
| [syssi/esphome-treadmill-f15](https://github.com/syssi/esphome-treadmill-f15) | Sportstech F15/F31, FitShow-shaped `02 51 … 03` status frames. | No |
| ESPHome-Treadmill-FTMS, pacekeeper, NordicTrack iFit bridges | Either UART-to-motor or a different proprietary BLE. | Unrelated |

**Guesses that failed the evidence bar:**

- “FAB0 is the LED service” — `0xFAB0` is a 16-bit vendor UUID with **zero** public command table. This repo already probed FitShow-style frames onto `FAB1`/`FAB2`: ATT-ACK, **no notify reply**.
- “WalkingPad LED frames will work” — KingSmith WiLink (`F7 A2 …`) is a different board family. Do not replay those onto the W2 Pro.
- Sperax P3 Max vibration opcodes (`0xFFF0`) — different firmware (`SPERAX_P3MAX`, wi-linktech WLT6200), not RM-01 / W2 Pro.

**Confirmed (this repo, 2026-07-05, not the public internet):** Sperax RM-01 pcap with identical GATT contained **only time-sync writes** on `d18d2c10-…`, no control, no LED.

---

## 2. Yesoul Fitness app — decompile / MITM / Chinese writeups

**Verdict: not found.** No public APK reverse, HCI/MITM capture, or command table naming `light` / `led` / `lamp` / `atmosphere` / `氛围灯` / `灯光` for the treadmill backlight.

Apps that actually exist:

| Channel | Package | Notes |
|---|---|---|
| Play Store (international) | `com.yesoulchina.international.bicycle` | “YESOUL FITNESS”, Yesoul Fitness Inc. Bike-centric listing; still the app the W2 Pro manual points at for BLE pairing. Latest tracked ~2.10.26 (2026-07). |
| China (应用宝 / 野小兽) | `yesoul.yesoulmobile` | 福建野小兽健康科技有限公司, v4.13.x as of 2026-08. This is the domestic app most likely to contain walking-pad vendor commands. |
| iOS | Apple ID `1517375015` | Same “YESOUL FITNESS” brand. |

Searches that returned **nothing on-protocol**:

- GitHub/Gitee/CSDN/知乎 for `野小兽` + `蓝牙` + (`灯光` \| `氛围灯` \| `LED`) + (`协议` \| `反编译`).
- APKMirror/writeup-style posts for `com.yesoul…` GATT writes.
- HCI dumps tagged Yesoul treadmill.

What *does* show up, and is a trap:

- Yesoul **bike** LED marketing (“Dynamic LED Lights activate when you pedal”) — bike-console RGB, different GATT (`0xFFF0` bike path).
- Yesoul T1M Plus / T3S Plus **display backlight** and “LED Light Feedback System” on walking-pad product pages — marketing copy for the **console LED strip**, not a protocol dump.
- Patent WO2017166709A1 (“Method and device for controlling ambient light of treadmill”) — a 2017 Xiaomi-era idea, not Yesoul firmware and not a BLE opcode table.
- Generic `com.ledlamp` / LEDBLE car-strip protocols (`7E FF 04 01 … EF` on `0xFFE0`) — consumer RGB controllers, unrelated.

**Practical implication:** the APK has never been publicly reversed for this command. Doing it yourself is still the highest-leverage next step (see §Recommendation). Expect some obfuscation; BLE UUIDs and Chinese UI strings (`灯光`, `氛围灯`, `灯带`) usually survive ProGuard.

---

## 3. FitShow protocol — is there a light opcode?

**Verdict: no, in every published command set through 2026.**

Primary source: FitShow (运动秀厦门) *智能跑步机蓝牙通讯协议* v1.1, 2017-08-20, still hosted at `fitshow.com/download/`. Frame:

```
02 | CMD | DATA… | FCS=XOR(CMD..DATA) | 03
```

GATT for the classic FitShow BLE module: service `0xFFF0`, notify `0xFFF1`, write `0xFFF2`.

Documented opcodes — **this is the complete control surface**:

| CMD | Name | Subcommands |
|---|---|---|
| `0x50` | `SYS_INFO` | model `00`, speed `02`, incline `03`, odometer `04` |
| `0x51` | `SYS_STATUS` | standby/end/start/running/stopping/error/disable/ready/paused |
| `0x52` | `SYS_DATA` | sport, info, program speed/incline |
| `0x53` | `SYS_CONTROL` | user `00`, ready `01`, target speed+incline `02`, stop `03`, program speed `04`, program incline `05`, start `09`, pause `0A` |

Unknown-but-well-formed commands are specified to be **echoed with `0x7F`**, not executed.

2026 clones do not add a light either. [Fit Monster treadmill BLE protocol v1.1 (2026-06-12)](https://www.fitmonster.club/SDK/FM-Protocol-Treadmill.html) is the same `02 … xor 03` dialect on `FFF0/FFF1/FFF2`, plus:

- `0x44 0xCC` — OTA mode (then switches to `FFE0` UART-passthrough)
- `0x60 0x0A` — module reboot (`02 60 0A 6A 03`)

Still no light/LED/lamp.

This repo already fired FitShow-shaped queries at W2 Pro `FFF2` / `FAB1` / `FAB2`: ATT-ACK, **never answered**. So even if a later unpublished FitShow dialect grew a light subcommand, **this firmware is not speaking FitShow on those pipes**. Treating LED as “just another `0x53` subcommand” is a guess that the live device has already falsified for incline.

`qdomyos-zwift` `fitshowtreadmill.cpp` implements the table above (start/stop/speed/incline, user-id handshake). No LED symbol, no extra opcode.

---

## 4. UUID `d18d2c10-c44c-11e8-a355-529269fb1459`

**Verdict: vendor unknown in public sources. No protocol doc. Strong circumstantial ID as an OEM-patched FTMS extra characteristic used at least for RTC/time-sync.**

Evidence, ranked:

1. **Hardware-confirmed here (2026-07):** write-only characteristic **inside** the standard FTMS service `0x1826`, not in a vendor service. Same tree on Sperax RM-01. DIS on the W2 Pro: model `W2PRO`, FW `SDC_W2_BT_V3.03-50-54`, manufacturer `YESOUL`.
2. **Sperax RM-01 pcap (this repo):** only **time-sync frames** on this UUID. No speed, no incline, no LED in that capture.
3. **UUID shape:** RFC 4122 version 1 (`…-11e8-…`), time field ~October 2018, node `52:92:69:fb:14:59` (locally-administered MAC — typical of `uuid.uuid1()` on a build machine, not a SIG-assigned number). This is how cheap Chinese BLE-module vendors mint 128-bit characteristics.
4. **Public search is a wash.** Verbatim Google/Bing hits are unrelated Windows GUIDs. GitHub code search requires login and returned empty; grep.app / Sourcegraph index hits: empty. No Nordic Infocenter, no FitShow PDF, no SIG GATT XML.

**What it is not:**

- Not a Bluetooth SIG characteristic (would be `0000xxxx-0000-1000-8000-00805f9b34fb`).
- Not Nordic UART (`6E40…`), not Nordic DFU (`FE59`), not KingSmith WiLink (`FE01`), not FitShow’s 16-bit `FFF2`.
- Not documented as SPAX/FitShow. SPAX in OEM manuals is the **consumer app brand** sitting next to FitShow (Xiamen 运动秀), i.e. software, not this 128-bit UUID.

**Working hypothesis (labelled as such):** an OEM FTMS stack (board string `SDC_W2_BT` — vendor still unidentified) stuffed extra write chars into `0x1826`. Time-sync is one opcode on `d18d2c10`. LED *might* be another opcode on the **same** char, or it might live on `FAB1`/`FAB2` (write-no-rsp, no public docs) or `FFF2` (FitShow-shaped, unanswered on this unit). Until a capture, all three remain candidates; `0xFF00`/`0xFF01` stays **do-not-write** (OTA/DFU-shaped, read value `4f 02 00 00`).

---

## 5. 2026 tooling to capture the app’s BLE writes (Xiaomi / HyperOS)

Goal: see the ATT Write that the Yesoul app issues when the operator toggles the LED strip. Ranked by expected time-to-frame on a Xiaomi phone.

### 5.1 Xiaomi / HyperOS HCI snoop (default path)

HyperOS still inherits the MIUI HCI-log quirks. Developer-options “Bluetooth HCI snoop log” **alone is often not enough** on Xiaomi.

**Recipe that Chinese 2025–2026 writeups still document:**

1. Developer options: USB debugging on; “Bluetooth HCI snoop log” / “蓝牙数据包日志” on; log buffer 16 MB; log level Verbose if present.
2. Toggle Bluetooth **off then on** (snoop often starts only on BT restart; some units need a reboot).
3. Dialer secret: `*#*#5959#*#*` — Xiaomi’s “enable BT logging” gate. First entry may show a privacy dialog; enter again until it says logging is on. After the session, enter the code **again** to freeze/dump the log.
4. Exercise: connect Yesoul app → toggle LED **≥5 times** with ~2 s pauses (matches backlog 004). Also do one speed change as a positive control so you can see what a known write looks like.
5. Pull the file. Xiaomi locations, in the order to try:

| Path | Root? | Notes |
|---|---|---|
| `/sdcard/MIUI/debug_log/common/` | no | `btsnoop_hci.log` or timestamped `hci_snoopYYYYMMDDHHMMSS.cfa` |
| `/sdcard/MIUI/debug_log/common/com.android.bluetooth/btsnoop_hci.log` | no | Android 11-era Xiaomi path, still cited |
| `adb bugreport captures/led.zip` then `FS/data/misc/bluetooth/logs/btsnoop_hci.log` | no | AOSP layout inside the zip |
| `FS/data/log/bt/btsnoop_hci.log` | no | some OEM bugreports |
| `/data/misc/bluetooth/logs/btsnoop_hci.log` | **yes** | live file; ignore unless rooted |

Open in Wireshark. Useful filters:

```
btatt.opcode == 0x12 || btatt.opcode == 0x52          # Write Request / Write Command
bluetooth.uuid == d18d2c10-c44c-11e8-a355-529269fb1459
btuuid == 0xfab1 || btuuid == 0xfab2 || btuuid == 0xfff2
```

Diff the five LED toggles against the speed-change control. The LED frame is the ATT write that appears on toggle and **not** on speed change.

**`adb bugreport` caveats (2026):**

- Must be `adb bugreport out.zip`, **not** `adb bugreport > out.txt` (the latter mangles the zip). Takes 30–120 s; it is a **snapshot**, so stop toggling, then dump immediately.
- The zip is huge and full of PII. Extract only the btsnoop path.
- On some HyperOS builds the snoop file is omitted from bugreport unless the `5959` dump ran first.
- **Android 16 killed live `btsnoop` TCP:8872** (Gabeldorsche, security-audit hold). Wireshark’s `androiddump` / “Android Bluetooth Btsnoop Net” interface does not work on unrooted Android 16. Android 17 restored it behind a second Developer-options toggle: **“Enable Bluetooth HCI snoop log socket”** (Bluetooth submenu). If the Xiaomi is on HyperOS 3 / Android 16, plan on bugreport/`5959` files, not live Wireshark.

Authoritative 2026 writeup: [Your Android Bluetooth Traffic Captures Should Be Live (Insinuator, 2026-07-27)](https://insinuator.net/2026/07/your-android-bluetooth-traffic-captures-should-be-live/). Xiaomi-specific dump path: [EET-China 汇总](https://www.eet-china.com/mp/a169598.html), [掘金 2026-03](https://juejin.cn/post/7615125935390507034), [SO Xiaomi A11](https://stackoverflow.com/questions/23877761/sniffing-logging-your-own-android-bluetooth-traffic).

### 5.2 Parallel: jadx the APK (no treadmill required)

Faster than a capture if the strings are intact:

```
# APK: yesoul.yesoulmobile (CN) or com.yesoulchina.international.bicycle (intl)
jadx -d yesoul-src yesoul.apk
rg -n '氛围灯|灯光|灯带|backlight|ledStrip|setLight|lamp' yesoul-src
rg -n 'd18d2c10|FAB1|fff2|writeCharacteristic' yesoul-src
```

Look for `BluetoothGatt.writeCharacteristic` call sites near those strings. Even obfuscated apps usually leave the UUID literals and a `byte[]` constructor. Frida is the fallback if jadx is soup:

```
Java.use('android.bluetooth.BluetoothGatt').writeCharacteristic.overload(...).implementation = function (c) {
  console.log(c.getUuid() + ' ' + hex(c.getValue()));
  return this.writeCharacteristic(c);
};
```

### 5.3 Alternatives if HyperOS HCI is wedged

| Tool | What you get | Cost / catch |
|---|---|---|
| **nRF Sniffer v4.1** + nRF52840 DK/dongle + Wireshark 4.x | Over-the-air ATT, including writes the phone never logs | Must follow the connection from `CONNECT_IND`; LE Secure Connections traffic is encrypted unless you also extract the LTK (PacketLogger on iOS can). Cheap, well documented 2026. |
| **Sniffle** (NCC Group) on a Sonoff Zigbee 3.0 USB (CC26x2) | Same OTA role, good Wireshark extcap | Linux-friendliest |
| **iOS PacketLogger** (Additional Tools for Xcode) | HCI on iPhone/iPad running Yesoul; export `.pklg` → Wireshark; can dump LTK | Only if the iOS app actually exposes the LED toggle |
| **Bumble GATT proxy** | Mac advertises the W2 Pro’s GATT; phone talks to the Mac; Mac forwards to the real treadmill | Needs a USB BLE dongle (macOS CoreBluetooth cannot be a proper peripheral + central MITM at once). Bumble `PcapSnooper` streams HCI into Wireshark as of v0.0.224. Google also ships an [Android Remote HCI](https://google.github.io/bumble/extras/android_remote_hci.html) APK that exposes the **phone’s** controller over TCP — useful, but it is the phone’s HCI, same as snoop. |
| **ChimeraBLE** (ESP32) / BtleJuice | Dedicated MITM | Extra hardware; BtleJuice is aging |
| **Ubertooth** | Last-resort OTA | BLE following is painful vs nRF Sniffer |

**Do not** use a Mac-only CoreBluetooth sniffer: macOS does not give you other-apps’ ATT payload without PacketLogger on the iOS side or a USB sniffer.

---

## Confirmed vs guess

| Claim | Status |
|---|---|
| Official Yesoul app can toggle the W2 Pro LED over BLE | Operator-verified (this repo’s premise). **No public frame.** |
| FTMS has no LED opcode | Confirmed (spec). |
| FitShow v1.1 / Fit Monster 2026 have no LED opcode | Confirmed from the PDFs. |
| W2 Pro does not answer FitShow frames on `FFF2`/`FAB1`/`FAB2` | Hardware-verified, this repo. |
| `d18d2c10-…` carries time-sync on Sperax RM-01 | Hardware-verified pcap, this repo. |
| `d18d2c10-…` is the LED char | **Guess.** Same char *could* hold a second opcode; equally likely `FAB1`/`FAB2`. |
| `0xFF00`/`0xFF01` is OTA | Guess, but strong (shape + this repo’s prior call). Do not write. |
| Board is FitShow/Sperax/SPAX OEM | Circumstantial (identical GATT, `SDC_W2_BT` string). SPAX = FitShow app brand, not a UUID. |
| Any OSS LED frame for this family | **Not found, 2026-08-28.** |

---

## Prioritized path to the LED frame

1. **jadx the China APK `yesoul.yesoulmobile` (and the international APK if the first is a stub).** Search `氛围灯` / `灯光` / `d18d2c10`. This is the only step that does not need the treadmill powered, and it often yields the exact `byte[]` in <1 hour. Treat decoded arrays as **candidates**, not confirmed, until a live write.
2. **Xiaomi HCI snoop of 5 LED toggles** (backlog 004, with the 2026 HyperOS amendments: `*#*#5959#*#*`, pull from `MIUI/debug_log/common/`, do **not** rely on live `androiddump` if the phone is Android 16). Positive-control: one `tm speed` / in-app speed change so FTMS `0x2AD9` writes are visible for contrast. Filter ATT writes, ignore `0xFF00`.
3. If the APK is obfuscated and HCI is empty/broken: **Frida `writeCharacteristic` hook** while tapping LED. This survives HyperOS snoop bugs because it is in-process.
4. If the phone will not log HCI at all: **nRF52840 Sniffer + Wireshark**, follow the Yesoul-app connection, same ATT filter. Optional: iOS PacketLogger if the iOS app has the toggle.
5. Bumble/ChimeraBLE MITM only if 1–4 fail (Mac-as-treadmill is slow to get GATT-perfect, and the Yesoul app may fingerprint the DIS/name).

**Do not:** brute-force writes to `0xFF00`/`0xFF01`; replay KingSmith `F7 A2` frames; assume FitShow `0x53` grew a light subcommand on this firmware.

Once a candidate frame is in hand, the implementation is a small vendor write next to `src/fitshow.rs` (new module, not a FitShow opcode — this device has already shown it is not speaking that dialect for vendor extras), gated on the `d18d2c10` / `FAB*` / `FFF2` handle the capture names, with a live on/off check against the strip.

---

## Sources (primary)

- This repo: `docs/research/gatt-snapshot.json`, `docs/research/001-yesoul-ble-protocol.md`, `docs/tasks/003-yesoul-w2-pro-controller.md`, `docs/backlog/004-led-control-via-hci-capture.md`, `src/fitshow.rs`
- FitShow protocol v1.1 PDF: https://fitshow.com/download/%E8%BF%90%E5%8A%A8%E7%A7%80%E7%94%B5%E8%B7%91%E8%93%9D%E7%89%99%E9%80%9A%E8%AE%AF%E5%8D%8F%E8%AE%AE.pdf
- FitShow BLE-module book (FFF0/FFF1/FFF2): https://fitshow.com/download/%E8%BF%90%E5%8A%A8%E7%A7%80%E8%93%9D%E7%89%99%E6%A8%A1%E5%9D%97%E5%BA%94%E7%94%A8%E8%AF%B4%E6%98%8E%E4%B9%A6.pdf
- Fit Monster protocol (2026-06): https://www.fitmonster.club/SDK/FM-Protocol-Treadmill.html
- qdomyos-zwift: https://github.com/cagnulein/qdomyos-zwift
- hass-walkingpad Sperax notes: https://github.com/mcdax/hass-walkingpad
- Insinuator 2026-07 live Android HCI: https://insinuator.net/2026/07/your-android-bluetooth-traffic-captures-should-be-live/
- Insinuator 2026-02 Bumble→Wireshark: https://insinuator.net/2026/02/capture-bumble-bluetooth-traffic-with-wireshark
- Bumble Android Remote HCI: https://google.github.io/bumble/extras/android_remote_hci.html
- nRF Sniffer + Wireshark 4.x (2026): https://hubble.com/community/guides/how-to-capture-ble-packets-with-nrf-sniffer-and-wireshark/
- iOS PacketLogger: https://novelbits.io/debugging-sniffing-secure-ble-ios/
- Xiaomi HCI paths: https://www.eet-china.com/mp/a169598.html · https://stackoverflow.com/questions/23877761/sniffing-logging-your-own-android-bluetooth-traffic
- YESOUL FITNESS Play listing: https://play.google.com/store/apps/details?id=com.yesoulchina.international.bicycle
- China Yesoul app package `yesoul.yesoulmobile` (应用宝 / 当快)

## Report

- **Changed:** nothing (read-only research; no repo files written).
- **Ran:** web/GitHub/CSDN/Gitee/FitShow-PDF searches for LED/light/氛围灯 on Yesoul, Sperax, SPAX, FitShow, `d18d2c10-…`, `FAB1`/`FAB2`, `FFF2`; fetched FitShow v1.1 + module book + Fit Monster 2026 protocol; checked qdomyos-zwift, hass-walkingpad, treadspan, ph4-walkingpad, ESPHome treadmill components; read local GATT snapshot, tasks 003, backlog 004, `src/fitshow.rs`; reviewed 2026 Android HCI (Insinuator A16/A17), Xiaomi `*#*#5959#*#*` + `MIUI/debug_log`, Bumble/nRF/PacketLogger.
- **Could not do:** GitHub code search of the UUID (requires login; grep.app/Sourcegraph empty); download/decompile the Yesoul APK (out of scope for read-only internet research); live HCI capture (needs the operator’s phone + treadmill).

## Unverified

- Exact HyperOS version / Android API on the operator’s Xiaomi (determines whether live `androiddump` works).
