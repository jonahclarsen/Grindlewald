# Grindlewald

Grindlewald is a small macOS menu-bar app for controlling Govee Bluetooth lights directly. It is built with Rust and Tauri, keeps connections warm while you adjust a color, and has both a visual controller and a scriptable CLI.

<p align="center">
  <img src="docs/screenshots/controller.png" width="31%" alt="Grindlewald color controller">
  <img src="docs/screenshots/automations.png" width="31%" alt="Grindlewald automation editor">
  <img src="docs/screenshots/settings.png" width="31%" alt="Grindlewald light settings">
</p>

## Highlights

- Native macOS menu-bar popover that anchors under its icon and hides when it loses focus
- A custom click-and-drag hue control for RGB mode and warm-to-cool slider for dedicated-white mode
- Dragging either control switches light mode immediately
- Configurable BLE connection hold time, making follow-up color changes fast
- Native H6005 white-temperature packets from 2000–9000 K
- A locally streamed rainbow party mode with instant H6005 transitions
- Breathing mode with adjustable 0.1–2 second timing and 0.1–120° hue steps, defaulting to 0.75 seconds and 2°
- A constrained experimental panel for trying scene and music-mode payloads on one light at a time
- Named color presets shared by the UI, CLI, and automations
- Compact preset and light rows that expand for editing and collapse when you click elsewhere
- Bluetooth discovery plus add, edit, enable, and remove controls for individual lights; existing matches remain visible and are labeled as already added
- Bluetooth identifiers are displayed and stored in uppercase but matched case-insensitively for discovery and live connections
- Daily local-time automations targeting one, several, or all enabled lights
- Optional trusted shell command run alongside an automation as the user or through a native macOS administrator prompt, with a full Test button
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

# Slowly breathe between colors; smaller hue steps make each transition subtler
grindlewaldctl breathe --pace 1 --hue-step 6
grindlewaldctl breathe --pace 2 --hue-step 18 --light Bedroom
grindlewaldctl stop-effect

# Experimental payload bytes after the fixed, safe 33 05 color/mode prefix
grindlewaldctl experiment '04 08' --light Bedroom

# Brightness and power
grindlewaldctl brightness 0.2
grindlewaldctl power off --light Bedroom
```

Brightness values range from `0.0` through `1.0`. Preset and light names are case-insensitive at execution time. You can add and edit presets in **Settings**.

## Automations

On **Automations**, create an automation, choose its daily local time and preset, then select any number of lights. Selecting no lights means all enabled lights. The scheduler runs inside the menu-bar process, so keep Grindlewald running.

An automation may also run a shell command through `/bin/zsh -lc`. Enable **Run as administrator** to have macOS request administrator authorization and execute that command as root. The authorization dialog appears when the automation runs, so unattended root automations instead need an explicitly configured, tightly scoped `sudo -n`/`sudoers` rule. Grindlewald never stores an administrator password.

The command is stored only in the local settings file. Use this only for commands you trust. **Test light + script now** saves the automation and runs both halves immediately, including the authorization prompt when enabled.

## Start automatically in live development mode

To install the included per-user LaunchAgent:

```sh
./scripts/install-launch-agent.sh
```

The installer creates a debug-mode `Grindlewald.app` container in `~/Applications` and registers a per-user LaunchAgent. The signed app acts as the development supervisor and keeps `pnpm tauri dev` running. Tauri watches the frontend and Rust sources; frontend edits reload automatically, while Rust edits rebuild and restart the executable inside the existing app container. You do not need to rerun the installer after source changes.

The development runner launches each build from inside the signed container, and the agent is associated with the Grindlewald bundle identifier. Bluetooth permission and the System Settings Background Items entry therefore use **Grindlewald** instead of a shell or generic development process. Logs are written under `~/Library/Logs/Grindlewald`. Re-run the installer only if the repository moves or the LaunchAgent scripts change.

Remove it with:

```sh
./scripts/uninstall-launch-agent.sh
```

## How the light protocol works

Grindlewald follows the same direct-BLE packet logic as the scripts that preceded it. Commands are padded to 19 bytes and followed by an XOR checksum. Writes use the Govee control characteristic and are sent without a response.

The two profiles deliberately encode white differently:

- **Classic / H6001:** mode `0x02`, `FF FF FF`, a dedicated-white flag, then the selected white RGB value.
- **H6005:** mode `0x0D`, RGB, a big-endian Kelvin value, then the same RGB again. The slider covers the captured 2000–9000 K range. This is not interchangeable with the Classic packet: H6005 can acknowledge an old-style packet while ignoring it.

The H6005 ordinary `0x0D` mode fades between colors, so party mode enters its instant `0x05` music stream once and then sends rainbow frames locally. Breathing mode does the opposite: each session starts at a random hue, advances by the selected 0.1–120° hue step at the selected 0.1–2 second pace, and lets the bulb produce its smooth native fade. The defaults are 2° every 0.75 seconds. A smaller hue step makes adjacent updates more alike and takes longer to complete a full color-wheel cycle. Classic lights receive the same sequence but may transition more abruptly. Choosing any normal control stops the active effect and restores ordinary control.

While the configured connection window is active, Grindlewald sends the captured `AA 01 … AB` no-op every two seconds. This keeps H6005 links alive beyond their roughly 15-second idle timeout without changing light state.

Protocol references: [H6005 write-up](https://github.com/egold555/Govee-Reverse-Engineering/blob/master/Products/H6005.md), [captured H6004/H6005 command set](https://github.com/egold555/Govee-Reverse-Engineering/blob/master/Products/H6004.md), [H6001/H6127 command set and scenes](https://github.com/egold555/Govee-Reverse-Engineering/blob/master/Products/H6127.md), and [classic H6001 controller](https://github.com/chvolkmann/govee_btled).

H6001 has documented built-in music, scene, and DIY packets. H6005 has confirmed instant music streaming but its scene and DIY payloads remain undocumented. The collapsed **Experimental modes** panel provides known H6001 candidates, some newer-Govee music candidates, and an editable hexadecimal payload. It always fixes the outer command to `33 05`, writes only the normal light-control characteristic, targets one selected light, and cannot access firmware or OTA characteristics. Use **Restore color** if a trial leaves a bulb in an unexpected mode.

## Privacy and repository safety

No device identifier, credential, API key, user automation, or personal filesystem path is included in this repository. The screenshots use synthetic demo devices. Before publishing changes, inspect staged files and keep all real configuration in Application Support.

## License

[MIT](LICENSE)
