# Govee app research

Findings from decompiling Govee Home 3.6.1 for the H6001 and H6005, plus what Grindlewald has and has not done with them. The README covers what is already shipped; this file covers how we know it and what could come next.

Evidence labels:

- **Hardware:** confirmed on real H6001 and H6005 lights by reading their state back.
- **Code:** read in the decompiled app, never sent to a light by Grindlewald.
- **Inference:** concluded from the code, not stated by it.

Everything is **Code** unless labeled otherwise. The app targets these models generally, but its behavior may differ from current firmware or newer app versions.

## Getting the app

Never commit an APK or decompiled source. Keep both in a temporary directory.

Govee Home 7.1.30 and newer is packed with Virbox: jadx sees a 3-class stub (`v68ae1757.l68ae1757`) and the real dex is encrypted. Older builds are the ones worth reading.

| Source | Result |
|---|---|
| Aptoide API | **Use this.** Serves unpacked 3.6.1 (2021, 15 dex files) |
| APKPure, through [apkeep](https://github.com/EFForg/apkeep) | Only 7.1.30 and newer |
| Uptodown | History back to 2021, but downloads are behind Cloudflare Turnstile |
| APKMirror | Cloudflare challenge for scripted clients |

```sh
# The download URL is at data.file.path
curl -s 'https://ws75.aptoide.com/api/7/app/getMeta/package_name=com.govee.home'
jadx -d <output-dir> <apk>   # jadx 1.5.6
```

Check the APK v2 signing certificate before trusting a copy. The 3.6.1 build and the current Play build both have SHA-256 `94257a44978717e568b00c24bfcc385ec2f1cfeaf42885bbeb3232c94907eb43`.

### Where to look

Paths are under `sources/com/govee/`. Class and method names are obfuscated and may change with a different jadx version.

| Path | Contents |
|---|---|
| `h6001/` | H6001: `ble/`, `adjust/` |
| `bulblightv2/` | H6005 (goods type 20): `ble/`, `adjust/v1/`, `adjust/Diy.java` |
| `base2light/ble/controller/` | Shared commands: timers, wake-up, sleep, sync time, brightness |
| `base2light/ble/comm/`, `base2light/ble/AbsBle.java` | Write queue, acknowledgements, keep-alive |
| `base2light/light/ModeStr.java` | Scene names |
| `base2light/ac/diy/DiyM.java` | DIY effect codes |
| `base2light/light/RecordController.java` | Microphone capture for music mode |
| `base2home/Constant.java` | Kelvin range and RGB table |

## Frames

All frames are 20 bytes: payload padded with zeros, then an XOR of the first 19 bytes.

| Prefix | Use |
|---|---|
| `33 <cmd> …` | Write |
| `AA <cmd> …` | Read. The reply starts `AA <cmd>`, with the payload at byte 2 |
| `A1 <cmd> …` | Multi-packet write |

## Modes: `33 05 <submode> …`

| Submode | Mode | Payload | Status |
|---|---|---|---|
| `02` | H6001 color or white | `RGB flag RGB`; white is `FF FF FF 01 RGB` | Hardware, shipped |
| `0D` | H6005 color or white | `RGB KH KL RGB`, Kelvin big-endian | Hardware, shipped |
| `04` | Scene | `<id>` | Hardware, shipped |
| `05` | Music | Enter: `01`. Stream: `00 RGB` | Hardware, used by party mode |
| `0A` | H6005 DIY | `<code>` | Hardware, shipped |

### Scene IDs

Each model's app screen offers these:

- **H6001:** Vivid `0C`, Reading `0D`, Relax `0E`, Romantic `07`, Breathe `0A`, Morning `00`, Sunset `01`, Colorful `0B`
- **H6005:** Reading `0D`, Fade `11`, Breathe `0A`, Romantic `07`, Candlelight `09`, Heartbeat `14`, Energetic `10`, Blinking `19`

**Hardware** readback (`AA 05 01` replies `04 <id>`) covered Breathe on both models, Sunset on the H6001, and Heartbeat and Candlelight on the H6005. The other IDs are **Code**.

The app's shared name table (`ModeStr.b`) names more IDs. These are not offered on either model's screen and are untested:

`02` Night, `03` Party, `04` Movie, `05` Dating, `06` Sweet, `08` Blinking, `0F` Snow Flake, `12` Forest, `13` Dream, `15` Crossing, `16` Chasing, `17` Bright, `18` Aurora

The H6005's Blinking `19` is not in that table.

### H6005 DIY upload

The H6001 has no DIY.

1. Start packet: `A1 02 00 <packet count> 00`
2. Data packets: `A1 02 <n> <up to 16 bytes>`, numbered from 1
3. End packet: `A1 02 FF`
4. Activate: `33 05 0A <code>`

The app waits 300 ms between packets. Reassembled data:

`code, effect, sub-effect, speed 0–100, color byte count, RGB…, mix length, mix pairs`

The app uses code `FE` for shared effects, and Grindlewald uses the same code. Effects take up to 8 colors:

| Effect | ID | Status |
|---|---|---|
| Gradual | `00` | Code |
| Jumping | `01` | Code |
| Blinking | `02` | Code |
| Breathe | `05` | Hardware (reports `0A FE`), shipped |
| Mix | `FF` | Code. Combines up to 4 effects |

`DiyM.EffectCode` lists more IDs (marquee `03`, music `04`, stream, chase), but the H6005's `Diy.java` support list is the authority for what it accepts.

### Music mode

The app samples the phone microphone at 44.1 kHz, reading at most 2205 samples per color frame. That is about 50 ms per frame (**Inference**), and possibly less on phones with a smaller minimum buffer. Frames identical to the last one are skipped. The rest are sent without waiting for an acknowledgement, and are dropped if the radio is busy.

Its brightness envelope jumps by 80 on a beat, capped at 180 (soft) or 254 (dynamic), then decays by 20–40 or 70–90 per frame, down to 60 or 20.

## Brightness and white

- **Brightness:** raw byte 20–254 on both models. Percent converts as `20 + (pct - 1) * 2.37`. The app never sends below 20 or above 254; Grindlewald can send the full 0–255 range.
- **Kelvin:** 2000–9000 K in steps of 100, with a 142-entry RGB table (`Constant.e`). Kelvin index `i` uses entry `2i`.
- **H6001 white:** sends only the table's RGB, no Kelvin.

## Status reads

| Read | Argument | Reply payload |
|---|---|---|
| `AA 01` power | | byte 0 not zero means on |
| `AA 04` brightness | | raw byte |
| `AA 05` mode | `01` | submode, then its layout |
| `AA 06` software version | | NUL-terminated ASCII |
| `AA 07` hardware version | `03` | `03`, then ASCII |
| `AA 0B` delay-off (H6001) | | enable, set H, set M, left H, left M |
| `AA 0A` on/off timer (H6001) | slot `00` or `01` | enable, on H, on M, off H, off M, slot, repeat |
| `AA 23` timers (H6005) | `FF` | `FF`, then 4 × (enable and type, H, M, repeat) |
| `AA 12` wake-up (H6005) | | enable, end brightness, H, M, repeat, duration |
| `AA 11` sleep (H6005) | | enable, start brightness, close time, current time |

Order on connect:

- **H6001:** software, hardware, sync time, delay-off, timers 0 and 1, power, brightness, mode. Firmware older than 1.01.20 skips the hardware read.
- **H6005:** software, hardware, sync time, timers, wake-up, sleep, brightness, power, mode.

## On-device schedules

None of this is implemented in Grindlewald. Writes use the read layouts above:

| Write | Payload | Notes |
|---|---|---|
| `33 09` sync time | `HH MM SS weekday` | Monday is 1, Sunday is 7. The app sends it on connect |
| `33 23` timer (H6005) | `slot, enable and type, H, M, repeat` | 4 slots. Enable and type: bit 7 enabled, bit 0 turns on rather than off |
| `33 0A` timer (H6001) | `enable, on H, on M, off H, off M, slot, repeat` | 2 slots, one on and off time each |
| `33 0B` delay-off (H6001) | `enable, H, M` | |
| `33 12` wake-up (H6005) | `enable, end brightness, H, M, repeat, duration` | Brightness 0–100 when read back, default 100. Duration 10–60 min, default 10 |
| `33 11` sleep (H6005) | `enable, start brightness, close time, current time` | Brightness 0–100, default 30. Close time up to 240 min, default 15 |

Repeat byte (`NumUtil.d` and `NumUtil.e`):

- Bit 7 clear means every day; the app writes `00` when all days are selected.
- Bit 7 set means bits 0–6 choose Monday to Sunday. `80` selects no days (**Inference:** runs once).

## How the app paces traffic

| Topic | Govee Home | Grindlewald |
|---|---|---|
| Ordinary writes | One at a time: send, then wait for the matching notification (byte 2 is 0). No fixed gap | Fixed minimum spacing, measured at 26 ms, or 47 ms after a color/white switch |
| Timeouts | 6 s for writes, which are then dropped. 3 s for reads, which are resent; more than 10 failures disconnects | |
| Retries | Only when Android's `writeCharacteristic()` returns false, every 100–200 ms | |
| Queue | A new user command clears pending ones, so the latest wins | Latest wins, no catch-up |
| Streaming | About 50 ms, no acknowledgement, identical frames skipped | Breathing interval 26–1,000 ms, default 350 ms |
| Sliders | Nothing sent while dragging. Brightness and color temperature send on release, hue 300 ms after release | Sent while dragging |
| Keep-alive | `AA 01 … AB` every 2 s, except during OTA and multi-packet uploads. The reply's byte 2 is power, and a change re-reads all state | Same frame and interval; reply not used |
| Connection | 60 s connect timeout, no MTU or connection-priority requests | |

Renaming a light is cloud-only; no Bluetooth command was found.

## Verified and not verified

**Hardware:** the scenes and DIY effect labeled above, at 5% dim orange on macOS, through direct readback and the live app's CLI.

**Not verified:**

- Whether scenes and Breathe color look right. Readback proves the mode changed, not the visual effect.
- DIY speed direction. Higher is assumed to be faster.
- Scenes and Breathe color on a physical Android device. Android shares the same Rust encoder and UI, but its emulator tests have no Bluetooth lights.
- Current firmware's response to anything labeled **Code**.

## Ideas

Roughly in order of value. None has been started.

1. **Schedules stored on the light.** Timers, delay-off, wake-up, and sleep would run while the Mac is off or asleep, unlike today's automations. Sync the clock first, and start with one read-only screen to confirm the layouts. The H6005 is the better first target, with 4 timers plus wake-up and sleep.
2. **Acknowledgement-based pacing.** Waiting for each write's notification, as the app does, could detect dropped frames instead of working around them with fixed spacing. The risk is added latency during streaming, so keep streams unacknowledged. Compare with `grindlewaldctl measure-step-interval`.
3. **A 50 ms breathing floor.** The app's fastest stream is about 50 ms, while our floor is 26 ms. Try 50 ms if breathing drops steps. Skipping identical frames is a cheap addition for party mode and breathing.
4. **More H6005 DIY effects.** Add Gradual, Jumping, Blinking, and Mix. `diy_breathe_frames` needs only the effect ID and mix bytes generalized.
5. **Real state from the lights.** Use the keep-alive's power byte and the status reads to show changes made by the remote or another app.
6. **Try the unlisted scene IDs.** The Experimental modes panel can send them today; add any that work to `Scene`.
7. **Brightness limits.** Decide whether to keep sending below 20. Values below the app's range are outside what Govee tests, so check low-end behavior on each model before changing the presets' mapping.
8. **Microphone music mode.** Stream colors from the Mac or phone microphone using the envelope above. Party mode already has the frame path.
9. **Slider sends.** Sending only on release is simpler, but Grindlewald's live dragging is better. Keep it unless it causes trouble.
10. **Version reads.** Show software and hardware versions in light details, and use them to tell firmware-specific bugs apart.
11. **Measure the H6001.** `DeviceProfile` spacing still reuses the H6005's values. This predates the APK work but belongs here too: see Minimum frame spacing in the README.

## Testing on real lights

- Use dim red-orange at 3–10% brightness, say roughly how long the light will flicker, and restore the saved selection afterward.
- A running Grindlewald holds its Bluetooth connections, so a separate test process can find no lights. Disconnect or quit it first, or test through `grindlewaldctl`, which goes through the app.
- Prefer reading state back with `AA 05 01` over assuming a write landed. Frames can be dropped silently.
- `grindlewaldctl` in `~/.cargo/bin` does not update with the app. Run `cargo install --path src-tauri --bin grindlewaldctl --force` to get new commands.
