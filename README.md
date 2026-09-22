# Grindlewald

Grindlewald is a small macOS menu-bar app for controlling Govee Bluetooth lights directly. It is built with Rust and Tauri, keeps connections warm while you adjust a color, and has both a visual controller and a scriptable CLI.

<p align="center">
  <img src="docs/screenshots/controller.png" width="400" alt="Grindlewald color controller">
</p>

## Highlights

- Native macOS menu-bar popover that anchors under its icon and hides when it loses focus
- One-click controls for the adjacent `shortcut_set_floodlights.py` automation
- A custom click-and-drag hue control for RGB mode and warm-to-cool slider for dedicated-white mode
- Dragging either control switches light mode immediately
- Configurable BLE connection hold time, making follow-up color changes fast
- Live Bluetooth connection status with one-click disconnect, cancelling pending changes and stopping streamed effects
- Native H6005 white-temperature packets from 2000–9000 K
- A locally streamed rainbow party mode with instant H6005 transitions
- Breathing mode with 1–100 integer RGB color steps and a whole-spectrum cycle duration, defaulting to step 1 and 10 minutes; perceptual timing and brightness compensation
- A constrained experimental panel for trying scene and music-mode payloads on one light at a time
- Named color presets shared by the UI, CLI, and automations
- Compact preset and light rows that expand for editing and collapse when you click elsewhere
- Bluetooth discovery plus add, edit, enable, and remove controls for individual lights; existing matches remain visible and are labeled as already added
- Bluetooth identifiers are displayed and stored in uppercase but matched case-insensitively for discovery and live connections
- Daily local-time automations targeting one, several, or all enabled lights
- Optional trusted shell commands run normally or unattended through one root-owned helper after per-command macOS approval
- Local Unix-socket CLI, so terminal commands benefit from the menu app's warm BLE connections too
- Keyboard navigation with ⌘1 for Control, ⌘2 for Automations, ⌘3 for Settings, and Escape to dismiss

## Setup

Requirements: macOS, Bluetooth, Rust, Node.js, and [pnpm](https://pnpm.io/).

```sh
pnpm install
pnpm tauri dev
```

Click the **G** menu-bar icon, open **Settings**, and choose **Discover**. Grindlewald chooses H6005 automatically when the advertised device name contains `H6005`; every other discovered light defaults to Classic. You can override the protocol at any time:

- **H6005** for H6005-series devices
- **Classic (H6001)** for the older Govee BLE packet mode

macOS will ask for Bluetooth permission the first time Grindlewald scans or connects. Device names and CoreBluetooth identifiers are saved to:

```text
~/Library/Application Support/com.jonahclarsen.grindlewald/settings.json
```

That file is local runtime data. It is ignored by Git and is never compiled into the app.

The Floodlights buttons run `shortcut_set_floodlights.py on` or `shortcut_set_floodlights.py off` from the parent Govee project. Grindlewald uses the `govee` Miniconda environment when it is available. Set `GRINDLEWALD_FLOODLIGHT_SCRIPT` or `GRINDLEWALD_FLOODLIGHT_PYTHON` in the app environment to override either path without storing machine-specific configuration in this repository.

Errors appear in a compact panel above the navigation without moving the controls. The panel shows a short explanation and a scrollable monospace preview positioned at the end of the error. **Show details** expands the preview; **Copy error** copies the complete original message, including the traceback. Errors stay available across pages and later status updates until dismissed or replaced by another error.

If floodlight control reports `Bad file descriptor` while connecting to TP-Link, check whether a firewall such as Little Snitch is blocking Grindlewald or its Python subprocess from reaching `wap.tplinkcloud.com` over HTTPS. A successful connection from Terminal alone does not verify network access for the menu-bar app.

## Command-line control

The menu-bar app must be running because `grindlewaldctl` sends commands to its private local socket. Build or install the CLI once:

```sh
cargo install --path src-tauri --bin grindlewaldctl --force
```

Examples:

```sh
# Apply named presets to every enabled light or one light by name
grindlewaldctl preset nighttime
grindlewaldctl preset nighttime --light "Studio lamp"

# Direct RGB and dedicated-white colors
grindlewaldctl color '#ff4500' --brightness 0.35
grindlewaldctl white '#ffd5ad' --brightness 0.7 --light Bedroom
grindlewaldctl white '#ffa957' --kelvin 2700 --light Bedroom

# Stream or stop a rainbow party effect
grindlewaldctl party
grindlewaldctl party --light Bedroom
grindlewaldctl stop-party

# Slowly breathe between colors; 1 is the smallest RGB change
grindlewaldctl breathe --cycle-seconds 600 --color-step 1
grindlewaldctl breathe --cycle-seconds 60 --color-step 10 --light Bedroom
grindlewaldctl stop-effect

# Experimental payload bytes after the fixed, safe 33 05 color/mode prefix
grindlewaldctl experiment '04 08' --light Bedroom

# Brightness and power
grindlewaldctl brightness 0.2
grindlewaldctl power off --light Bedroom
```

Brightness values range from `0.0` through `1.0`. Preset and light names are case-insensitive at execution time. You can add and edit presets in **Settings**.

## Automations

On **Automations**, create an automation, choose its daily local time and preset, then select any number of lights. Selecting no lights means all enabled lights. Each automation can also turn the floodlights on or off, or leave them unchanged. The scheduler runs inside the menu-bar process, so keep Grindlewald running.

Use **Disable for** at the top of an enabled automation to pause it for **1–8 days**. Each day is exactly 24 hours, including across daylight-saving changes. The saved resume time survives app restarts; afterward the automation resumes its daily schedule without replaying missed runs. Its checkbox turns it back on early. Manually disabled automations stay off and do not offer the dropdown.

Floodlights offer **Turn on if on home network**, **Turn on**, and **Turn off**. Click the home-network option to save the current Wi-Fi router; click it again to replace the saved network. The saved identity survives page visits and app restarts. Selecting another option clears it, and opening Automations refreshes the current-network preview without changing a saved selection. At execution time, floodlights turn on only when the router matches; a different or unavailable network leaves them unchanged. Timestamped home-network decisions and successful floodlight script completions are written to the app log, without router identifiers. These decisions are logged independently of Bluetooth commands, so a light connection failure does not hide the floodlight outcome.

This check works with Location Services off. It discovers the Wi-Fi interface, reads its own IPv4 default router and ARP entry, and stores a SHA-256 fingerprint of the router’s hardware address in local settings. An empty ARP cache triggers one short probe to the local router. It does not read the SSID, use a public-IP service, or request Location permission. Networks sharing the same router identity count as the same home network; replacing the router requires saving it again. IPv6-only networks are not currently supported. Old SSID-based selections require clicking the option once to save the router. Existing “Do nothing” automations remain unchanged until you choose an option.

An automation may also run a shell command through `/bin/zsh -lc`. For unattended root commands, open **Settings** and install the **Privileged automations** service. macOS asks for administrator authorization once to install a root-owned copy of the Grindlewald helper and its narrowly scoped policy. Grindlewald never receives or stores the password. If the installed service needs an update or repair, the menu bar icon becomes a large bright orange warning triangle and a notice stays pinned at the top of **Control** while scrolling, with an action button to address it immediately. The normal icon returns after the helper is updated, repaired, or removed.

Enable **Run unattended as administrator** on an automation and press **Approve root command**. macOS asks you to approve that exact command, then the helper stores a root-only copy under `/Library/Application Support/Grindlewald/PrivilegedJobs`. At execution time the app sends only the automation's validated ID; it cannot substitute new command text. Editing the command makes the approval stale and prevents it from running as root until you approve it again. Any number of jobs share the same helper, and approvals survive helper updates.

The helper runs approved commands through `/bin/zsh -lc` with a clean root environment and a fixed executable search path. Referenced scripts must not be writable by an untrusted account: approving a command that invokes a user-editable script also approves whatever that script contains later. Use **Revoke** to remove one job or **Remove privileged service** to delete the helper, its sudo policy, and every root-owned job definition. **Test automation now** runs the preset, floodlight action, and script immediately using the same paths as the scheduler.

The unattended sudo policy is limited to the helper plus an argument expression equivalent to `run <letters-numbers-or-hyphens>`. Installation, approval, revocation, and removal are not covered by that policy and always go back through macOS authorization. Any process already running as your macOS account could request execution of a known approved job ID, but it cannot change that job or supply new root command text; revoke jobs whose fixed operation would be unsafe to trigger unexpectedly.

The installer verifies the copied executable against the app's SHA-256 digest, then applies and verifies a standalone ad-hoc signature before installing the root-owned helper. The app keeps its Developer ID signature. A root-owned receipt records both digests so update checks remain accurate after re-signing. Helpers installed before this receipt was introduced need a one-time repair through **Privileged automations**; existing job approvals are preserved.

## Start automatically in live development mode

To install the included per-user LaunchAgent:

```sh
./scripts/install-launch-agent.sh
```

The installer creates a debug-mode `Grindlewald.app` container in `~/Applications` and registers a per-user LaunchAgent. The signed app acts as the development supervisor and keeps `pnpm tauri dev` running. Tauri watches the frontend and Rust sources; frontend edits reload automatically, while Rust edits rebuild and restart the executable inside the existing app container. You do not need to rerun the installer after source changes.

The development runner launches each build from inside the signed container, and the agent is associated with the Grindlewald bundle identifier. Bluetooth permission and the System Settings Background Items entry therefore use **Grindlewald** instead of a shell or generic development process. Logs are written under `~/Library/Logs/Grindlewald`. Re-run the installer only if the repository moves or the LaunchAgent scripts change.

Development builds use a **Developer ID Application** certificate from your login keychain, including every rebuild. The first successful signing saves the certificate fingerprint in `~/Library/Application Support/Grindlewald/dev-signing-identity`; subsequent builds require that certificate and never fall back to ad-hoc signing. If multiple certificates are available, set `GRINDLEWALD_SIGNING_IDENTITY` to the desired SHA-1 fingerprint reported by `security find-identity -v -p codesigning` when running the installer. The saved selection also applies to unattended rebuilds. Local development builds are not notarized.

Moving from ad-hoc signing to Developer ID signing changes the app identity once, so Little Snitch and macOS permissions may need one more approval. Later rebuilds keep the same signing identity and bundle identifier.

Remove it with:

```sh
./scripts/uninstall-launch-agent.sh
```

## How the light protocol works

Grindlewald follows the same direct-BLE packet logic as the scripts that preceded it. Commands are padded to 19 bytes and followed by an XOR checksum. Writes use the Govee control characteristic and are sent without a response.

The two profiles deliberately encode white differently:

- **Classic / H6001:** mode `0x02`, `FF FF FF`, a dedicated-white flag, then the selected white RGB value.
- **H6005:** mode `0x0D`, RGB, a big-endian Kelvin value, then the same RGB again. The slider covers the captured 2000–9000 K range. This is not interchangeable with the Classic packet: H6005 can acknowledge an old-style packet while ignoring it.

The H6005 ordinary `0x0D` mode fades between colors, so party mode enters its instant `0x05` music stream once and then sends rainbow frames locally. Breathing mode starts at a random color-wheel position and lets the bulb produce its native fade. Classic lights receive the same sequence but may transition more abruptly. Choosing a normal control stops the active effect and restores ordinary control.

**Color step** is an integer from 1 to 100, defaulting to 1. One step changes one RGB channel by exactly 1 on its 0–255 scale. The wheel contains 1,530 distinct positions in the order red → yellow → green → cyan → blue → magenta. Larger steps skip positions. Each lap returns exactly to its initial color; when the step does not divide 1,530, the final step is shorter.

**Full cycle** controls the target time for one complete lap, in whole seconds, defaulting to 10 minutes and allowing up to 60 minutes. For step `s`, the number of updates is `N = ceil(1530 / s)` and the average interval is `cycleSeconds / N`. The minimum duration is `ceil(N × 0.3)` seconds: 7m 39s at step 1, 46s at step 10, and 5s at step 100. When Full cycle is at its minimum, changing Color step keeps it at the new minimum. Otherwise the selected duration is retained unless the new minimum requires a longer cycle. These duration bounds remain enforced in settings and the CLI, but individual perceptually weighted intervals can be shorter than 300 ms. Slow Bluetooth writes can lengthen an actual cycle; writes remain serialized instead of accumulating an overdue queue.

Older settings migrate automatically: the previous default step 9 becomes 1, steps above 100 clamp to 100, and the old default combination becomes a 10-minute cycle. Custom per-frame timing converts to a whole-cycle duration at the migrated step, rounded up and bounded by the minimum cycle duration. The CLI still accepts legacy `--pace` and `--hue-step`; degree steps are rounded and clamped to the new range. New commands use `--cycle-seconds` and `--color-step`.

Breathing now allocates time in proportion to Oklab color distance, using an explicit sRGB/linear-brightness approximation and the actual quantized brightness values sent to the lights. Darker-looking colors receive a brightness boost toward the modeled lightness of yellow, capped at 100% and never below the selected brightness packet. At 100%, every color remains at 100%; equal lightness is then impossible without dimming. Brightness packets are sent only when their 8-bit value changes, plus required synchronization. Stopping breathing restores the selected static brightness.

The normal Color hue slider and swatch show the last successfully sent breathing color. Dragging or using its arrow keys seeks within the running effect, immediately applies the latest requested hue after any in-flight write, and continues breathing from there. The selected static color is retained for when breathing stops. [Perceptual timing and brightness models](docs/perceptual-timing.md) documents the implemented equations, scientific sources, exact peak update rates, calibration assumptions, and validation.

The connection row above every page shows the current Bluetooth link status. Click the **Disconnect** button beside the connection status (also available while connecting) to release all light connections immediately and stop streamed effects without sending a power-off command. The status returns to **Disconnected** when the hold window expires. Using a light control again reconnects automatically; future scheduled automations still run. Reconnection scans start only when a command needs a disconnected light and stop as soon as all requested lights advertise, with a 1.4-second maximum discovery window instead of a fixed wait. There is no idle scanning or reconnect polling, and this does not extend the configured connection hold time.

While the configured connection window is active, Grindlewald sends the captured `AA 01 … AB` no-op every two seconds. This keeps H6005 links alive beyond their roughly 15-second idle timeout without changing light state.

Protocol references: [H6005 write-up](https://github.com/egold555/Govee-Reverse-Engineering/blob/master/Products/H6005.md), [captured H6004/H6005 command set](https://github.com/egold555/Govee-Reverse-Engineering/blob/master/Products/H6004.md), [H6001/H6127 command set and scenes](https://github.com/egold555/Govee-Reverse-Engineering/blob/master/Products/H6127.md), and [classic H6001 controller](https://github.com/chvolkmann/govee_btled).

H6001 has documented built-in music, scene, and DIY packets. H6005 has confirmed instant music streaming but its scene and DIY payloads remain undocumented. The collapsed **Experimental modes** panel provides known H6001 candidates, some newer-Govee music candidates, and an editable hexadecimal payload. It always fixes the outer command to `33 05`, writes only the normal light-control characteristic, targets one selected light, and cannot access firmware or OTA characteristics. Use **Restore color** if a trial leaves a bulb in an unexpected mode.

## Privacy and repository safety

No device identifier, credential, API key, user automation, or personal filesystem path is included in this repository. The screenshots use synthetic demo devices. Before publishing changes, inspect staged files and keep all real configuration in Application Support.

## License

[MIT](LICENSE)
