const invoke = window.__TAURI__?.core?.invoke;
const demoMode = !invoke;

const demoSettings = {
  devices: [
    { name: "Studio lamp", identifier: "LOCAL-BLE-ID-1", profile: "h6005", enabled: true },
    { name: "Bedroom", identifier: "LOCAL-BLE-ID-2", profile: "classic", enabled: true },
  ],
  color: "#ff4f22",
  white: "#ffd5ad",
  brightness: 0.4,
  connectionHoldSeconds: 6,
  breathingPaceSeconds: 0.75,
  breathingHueStepDegrees: 2,
  presets: [
    { name: "daytime", mode: "white", value: "#d6e1ff", brightness: 1 },
    { name: "eveningtime", mode: "white", value: "#ff8912", brightness: 0.35 },
    { name: "nighttime", mode: "color", value: "#ff4500", brightness: 0.35 },
    { name: "nighttimedark", mode: "color", value: "#ff4500", brightness: 0.03 },
    { name: "crashtime", mode: "color", value: "#ff4500", brightness: 0 },
  ],
  schedules: [
    { id: "demo-evening", name: "Evening wind-down", time: "21:30", enabled: true, lights: ["Studio lamp", "Bedroom"], preset: "eveningtime", floodlights: "off", shellCommand: "shortcuts run 'Wind Down'", runAsAdministrator: true, privilegedApprovedCommand: "shortcuts run 'Wind Down'", privilegedApprovedAt: "2026-09-04T08:00:00-07:00" },
  ],
};

let settings;
let discovered = [];
let pendingControl = null;
let sendingControl = false;
let activeEffect = null;
let expandedEditorKey = null;
let privilegedService = demoMode
  ? { installed: true, healthy: true, current: true, message: "Ready for unattended administrator jobs" }
  : { installed: false, healthy: false, current: false, message: "Not installed" };

const $ = (selector) => document.querySelector(selector);
const escapeHtml = (value) => String(value).replace(/[&<>'"]/g, (character) => ({
  "&": "&amp;", "<": "&lt;", ">": "&gt;", "'": "&#39;", '"': "&quot;",
})[character]);
const canonicalIdentifier = (value) => String(value).trim().toLocaleUpperCase();
const normalizeIdentifier = canonicalIdentifier;
const protocolForDeviceName = (name) => String(name).toLocaleLowerCase().includes("h6005") ? "h6005" : "classic";
const configuredDeviceFor = (discoveredDevice) => settings.devices.find(
  (configuredDevice) => normalizeIdentifier(configuredDevice.identifier) === normalizeIdentifier(discoveredDevice.identifier),
);

function hexToRgb(value) {
  const hex = value.replace("#", "");
  return [0, 2, 4].map((offset) => Number.parseInt(hex.slice(offset, offset + 2), 16));
}

function rgbToHex(rgb) {
  return `#${rgb.map((channel) => Math.round(channel).toString(16).padStart(2, "0")).join("")}`;
}

function hueFromHex(value) {
  const [red, green, blue] = hexToRgb(value).map((channel) => channel / 255);
  const maximum = Math.max(red, green, blue);
  const minimum = Math.min(red, green, blue);
  const difference = maximum - minimum;
  if (difference === 0) return 0;
  let hue;
  if (maximum === red) hue = ((green - blue) / difference) % 6;
  else if (maximum === green) hue = (blue - red) / difference + 2;
  else hue = (red - green) / difference + 4;
  return (hue * 60 + 360) % 360;
}

function colorAtHue(hue) {
  const sector = ((hue % 360) + 360) % 360 / 60;
  const intermediate = Math.round(255 * (1 - Math.abs((sector % 2) - 1)));
  const colors = [
    [255, intermediate, 0], [intermediate, 255, 0], [0, 255, intermediate],
    [0, intermediate, 255], [intermediate, 0, 255], [255, 0, intermediate],
  ];
  return rgbToHex(colors[Math.floor(sector) % 6]);
}

function mixRgb(start, end, amount) {
  return start.map((channel, index) => channel + (end[index] - channel) * amount);
}

const whiteAnchors = [
  [2000, [255, 141, 11]],
  [2700, [255, 169, 87]],
  [5500, [255, 238, 222]],
  [7500, [238, 239, 255]],
  [9000, [217, 225, 255]],
];

function kelvinAtPosition(position) {
  return Math.round(2000 + Math.max(0, Math.min(1, position)) * 7000);
}

function whiteAtPosition(position) {
  const kelvin = kelvinAtPosition(position);
  const upperIndex = whiteAnchors.findIndex(([anchorKelvin]) => anchorKelvin >= kelvin);
  if (upperIndex <= 0) return rgbToHex(whiteAnchors[0][1]);
  const [upperKelvin, upperRgb] = whiteAnchors[upperIndex];
  const [lowerKelvin, lowerRgb] = whiteAnchors[upperIndex - 1];
  return rgbToHex(mixRgb(lowerRgb, upperRgb, (kelvin - lowerKelvin) / (upperKelvin - lowerKelvin)));
}

function whitePositionFromHex(value) {
  const target = hexToRgb(value);
  let bestPosition = 0;
  let bestDistance = Number.POSITIVE_INFINITY;
  for (let step = 0; step <= 100; step += 1) {
    const candidate = hexToRgb(whiteAtPosition(step / 100));
    const distance = candidate.reduce((sum, channel, index) => sum + (channel - target[index]) ** 2, 0);
    if (distance < bestDistance) {
      bestDistance = distance;
      bestPosition = step / 100;
    }
  }
  return bestPosition;
}

function updateHue(hue, shouldSend = true) {
  const normalizedHue = ((hue % 360) + 360) % 360;
  settings.color = colorAtHue(normalizedHue);
  $("#hue-knob").style.setProperty("--knob-position", `${normalizedHue / 360 * 100}%`);
  $("#color-swatch").style.background = settings.color;
  $("#hue-track").setAttribute("aria-valuenow", String(Math.round(normalizedHue)));
  if (shouldSend) queueControl({ command: "color", value: settings.color, brightness: settings.brightness, device: null });
}

function updateWhite(position, shouldSend = true) {
  const normalizedPosition = Math.max(0, Math.min(1, position));
  settings.white = whiteAtPosition(normalizedPosition);
  $("#white-knob").style.setProperty("--knob-position", `${normalizedPosition * 100}%`);
  $("#white-swatch").style.background = settings.white;
  $("#white-track").setAttribute("aria-valuenow", String(Math.round(normalizedPosition * 100)));
  if (shouldSend) queueControl({ command: "white", value: settings.white, kelvin: kelvinAtPosition(normalizedPosition), brightness: settings.brightness, device: null });
}

function makeDraggable(track, update) {
  const applyPointer = (event) => {
    const bounds = track.getBoundingClientRect();
    update(Math.max(0, Math.min(1, (event.clientX - bounds.left) / bounds.width)));
  };
  track.addEventListener("pointerdown", (event) => {
    track.setPointerCapture(event.pointerId);
    applyPointer(event);
  });
  track.addEventListener("pointermove", (event) => {
    if (track.hasPointerCapture(event.pointerId)) applyPointer(event);
  });
  track.addEventListener("pointerup", (event) => {
    if (track.hasPointerCapture(event.pointerId)) track.releasePointerCapture(event.pointerId);
    save();
  });
}

function setStatus(message, kind = "ready") {
  $("#status").textContent = message;
  $("#connection-dot").className = kind === "ready" ? "" : kind;
}

function setDiscoveryBusy(isBusy) {
  const button = $("#discover-button");
  button.disabled = isBusy;
  button.textContent = isBusy ? "Scanning…" : "Discover";
  button.setAttribute("aria-busy", String(isBusy));
  if (isBusy) {
    $("#discovered-devices").innerHTML = '<div class="discovery-progress"><span class="spinner"></span><span>Looking for nearby lights…</span></div>';
  }
}

function setFloodlightsBusy(isBusy) {
  document.querySelectorAll("[data-floodlights]").forEach((button) => {
    button.disabled = isBusy;
    button.setAttribute("aria-busy", String(isBusy));
  });
}

async function call(command, args = {}) {
  if (demoMode) {
    if (command === "get_settings") return structuredClone(demoSettings);
    if (command === "discover_lights") return [
      { name: "Govee H6005", identifier: "local-ble-id-1" },
      { name: "Govee new lamp", identifier: "LOCAL-DEMO-ID" },
    ];
    if (command === "set_floodlights") return `Floodlights ${args.on ? "on" : "off"}`;
    if (command === "privileged_service_status") return privilegedService;
    if (command === "install_privileged_service") {
      privilegedService = { installed: true, healthy: true, current: true, message: "Ready for unattended administrator jobs" };
      return "Privileged automation service installed";
    }
    if (command === "uninstall_privileged_service") {
      privilegedService = { installed: false, healthy: false, current: false, message: "Not installed" };
      return "Privileged automation service removed";
    }
    if (command === "approve_privileged_job") return "Administrator command approved for unattended use";
    if (command === "revoke_privileged_job") return "Administrator job approval revoked";
    return command === "test_schedule" ? "Updated 2 light(s). Shell command completed." : "Updated 2 light(s)";
  }
  return invoke(command, args);
}

async function save() {
  try {
    await call("save_settings", { settings });
    setStatus("Saved");
  } catch (error) {
    setStatus(String(error), "error");
  }
}

async function queueControl(command) {
  setEffectActive(null);
  pendingControl = command;
  if (sendingControl) return;
  sendingControl = true;
  setStatus("Connecting…", "busy");
  while (pendingControl) {
    const latest = pendingControl;
    pendingControl = null;
    try {
      const message = await call("execute_control", { command: latest });
      setStatus(message);
    } catch (error) {
      setStatus(String(error), "error");
    }
  }
  sendingControl = false;
}

function setEffectActive(effect) {
  activeEffect = effect;
  [["#party-button", "party", "Party"], ["#breathing-button", "breathe", "Breathe"]].forEach(([selector, name, label]) => {
    const button = $(selector);
    const active = effect === name;
    button.classList.toggle("active", active);
    button.setAttribute("aria-pressed", String(active));
    button.textContent = active ? `Stop ${label.toLowerCase()}` : label;
  });
}

function showPage(pageId) {
  document.querySelectorAll(".page").forEach((page) => page.classList.toggle("active", page.id === pageId));
  document.querySelectorAll(".nav-item").forEach((item) => item.classList.toggle("active", item.dataset.page === pageId));
}

function renderQuickPresets() {
  $("#quick-presets").innerHTML = settings.presets.length
    ? settings.presets.map((preset) => `<button class="preset-chip" data-preset="${escapeHtml(preset.name)}"><span class="swatch" style="background:${escapeHtml(preset.value)}"></span>${escapeHtml(preset.name)}</button>`).join("")
    : '<div class="empty">Add a preset to get started.</div>';
}

function renderPresets() {
  $("#preset-editor").innerHTML = settings.presets.length ? settings.presets.map((preset, index) => {
    const editorKey = `preset:${index}`;
    if (expandedEditorKey !== editorKey) {
      return `<button class="compact-editor-card" data-expand-editor="${editorKey}"><span class="swatch" style="background:${escapeHtml(preset.value)}"></span><span class="summary-copy"><strong>${escapeHtml(preset.name || "Untitled preset")}</strong><small>${preset.mode === "color" ? "Color" : "White"} · ${Math.round(preset.brightness * 100)}%</small></span><span class="disclosure">›</span></button>`;
    }
    const colorPosition = preset.mode === "color"
      ? Math.round(hueFromHex(preset.value))
      : Math.round(whitePositionFromHex(preset.value) * 100);
    const colorClass = preset.mode === "color" ? "preset-hue-range" : "preset-white-range";
    const colorMaximum = preset.mode === "color" ? 360 : 100;
    return `
    <article class="editor-card expanded-editor" data-editor-key="${editorKey}" data-preset-index="${index}">
      <div class="card-title collapsible-card-title" data-collapse-editor-header><strong>${escapeHtml(preset.name || "Untitled preset")}</strong><span><button class="remove-button" data-remove-preset="${index}">Remove</button><button class="collapse-button" data-collapse-editor aria-label="Collapse preset">›</button></span></div>
      <div class="field-grid">
        <label class="field">Name<input data-preset-field="name" value="${escapeHtml(preset.name)}"></label>
        <label class="field">Mode<select data-preset-field="mode"><option value="color" ${preset.mode === "color" ? "selected" : ""}>Color</option><option value="white" ${preset.mode === "white" ? "selected" : ""}>White</option></select></label>
        <label class="field">${preset.mode === "color" ? "Hue" : "Warmth"}<input class="preset-color-range ${colorClass}" type="range" min="0" max="${colorMaximum}" data-preset-color value="${colorPosition}"></label>
        <label class="field">Brightness <span>${Math.round(preset.brightness * 100)}%</span><input type="range" min="0" max="100" value="${Math.round(preset.brightness * 100)}" data-preset-field="brightness"></label>
      </div>
    </article>`;
  }).join("") : '<div class="empty">No presets yet. Add one with ＋.</div>';
}

function renderSchedules() {
  const presetOptions = settings.presets.map((preset) => `<option value="${escapeHtml(preset.name)}">${escapeHtml(preset.name)}</option>`).join("");
  $("#schedule-editor").innerHTML = settings.schedules.length ? settings.schedules.map((schedule, index) => {
    const floodlightAction = schedule.floodlights || "unchanged";
    const approved = Boolean(schedule.privilegedApprovedCommand) && schedule.privilegedApprovedCommand === schedule.shellCommand.trim();
    const hasStaleApproval = Boolean(schedule.privilegedApprovedCommand) && !approved;
    const approveLabel = !privilegedService.installed
      ? "Install service first"
      : !privilegedService.healthy
        ? "Repair service first"
        : approved ? "Approve again" : "Approve root command";
    const approvalControls = schedule.runAsAdministrator || schedule.privilegedApprovedCommand ? `
      <div class="approval-row field full">
        ${schedule.runAsAdministrator ? `<button class="${approved ? "secondary" : "primary"} compact" data-approve-privileged="${escapeHtml(schedule.id)}">${approveLabel}</button>` : ""}
        ${schedule.privilegedApprovedCommand ? `<button class="text-button" data-revoke-privileged="${escapeHtml(schedule.id)}">Revoke</button>` : ""}
        <span class="approval-state ${approved ? "" : "pending"}">${approved ? "Approved for unattended use" : hasStaleApproval ? "Command changed · approval required" : "Approval required"}</span>
      </div>` : "";
    const warning = schedule.runAsAdministrator
      ? approved
        ? "Runs unattended through the root-owned Grindlewald helper. Editing the command requires approval again."
        : "This command will not run as root until you approve its exact contents with macOS."
      : schedule.privilegedApprovedCommand
        ? "A dormant administrator approval remains. Revoke it if you no longer need it."
        : "Runs locally through <code>/bin/zsh -lc</code>.";
    return `
    <article class="editor-card" data-schedule-index="${index}">
      <div class="card-title"><label class="inline"><input type="checkbox" data-schedule-field="enabled" ${schedule.enabled ? "checked" : ""}><strong>${escapeHtml(schedule.name || "Untitled automation")}</strong></label><button class="remove-button" data-remove-schedule="${index}">Remove</button></div>
      <div class="field-grid">
        <label class="field">Name<input data-schedule-field="name" value="${escapeHtml(schedule.name)}"></label>
        <label class="field">Every day at<input type="time" data-schedule-field="time" value="${escapeHtml(schedule.time)}"></label>
        <label class="field full">Preset<select data-schedule-field="preset">${presetOptions.replace(`value="${escapeHtml(schedule.preset)}"`, `value="${escapeHtml(schedule.preset)}" selected`)}</select></label>
        <div class="field full">Lights <span class="check-row">${settings.devices.map((device) => `<label class="check-pill"><input type="checkbox" data-schedule-light="${escapeHtml(device.name)}" ${schedule.lights.includes(device.name) ? "checked" : ""}>${escapeHtml(device.name)}</label>`).join("") || "No lights configured"}</span><small>None selected means all enabled lights.</small></div>
        <div class="field full">Floodlights <span class="radio-row" role="radiogroup" aria-label="Floodlights"><label class="radio-pill"><input type="radio" name="floodlights-${escapeHtml(schedule.id)}" value="on" data-schedule-field="floodlights" ${floodlightAction === "on" ? "checked" : ""}>Turn on</label><label class="radio-pill"><input type="radio" name="floodlights-${escapeHtml(schedule.id)}" value="off" data-schedule-field="floodlights" ${floodlightAction === "off" ? "checked" : ""}>Turn off</label><label class="radio-pill"><input type="radio" name="floodlights-${escapeHtml(schedule.id)}" value="unchanged" data-schedule-field="floodlights" ${floodlightAction === "unchanged" ? "checked" : ""}>Do nothing</label></span></div>
        <label class="field full">Optional shell command<textarea data-schedule-field="shellCommand" placeholder="shortcuts run 'Wind Down'">${escapeHtml(schedule.shellCommand)}</textarea></label>
        <label class="check-pill administrator-check"><input type="checkbox" data-schedule-field="runAsAdministrator" ${schedule.runAsAdministrator ? "checked" : ""}>Run unattended as administrator</label>
        ${approvalControls}
      </div>
      <div class="warning">${warning} Only approve commands you trust. Referenced script files can change without reapproval.</div>
      <button class="secondary" data-test-schedule="${escapeHtml(schedule.id)}">Test automation now</button>
    </article>`;
  }).join("") : '<div class="empty">No automations yet. Add one with ＋.</div>';
}

function renderPrivilegedService() {
  const title = privilegedService.installed
    ? !privilegedService.healthy ? "Repair required" : privilegedService.current ? "Ready" : "Update required"
    : "Not installed";
  $("#privileged-service-title").textContent = title;
  $("#privileged-service-message").textContent = privilegedService.message;
  $("#privileged-service-dot").classList.toggle("off", !privilegedService.installed || !privilegedService.healthy || !privilegedService.current);
  $("#privileged-service-install").textContent = privilegedService.installed
    ? privilegedService.healthy ? "Update" : "Repair"
    : "Install";
  $("#privileged-service-remove").hidden = !privilegedService.installed;
}

async function refreshPrivilegedService() {
  privilegedService = await call("privileged_service_status");
  renderPrivilegedService();
}

function renderDevices() {
  $("#device-editor").innerHTML = settings.devices.length ? settings.devices.map((device, index) => {
    const editorKey = `device:${index}`;
    if (expandedEditorKey !== editorKey) {
      return `<button class="compact-editor-card" data-expand-editor="${editorKey}"><span class="enabled-dot ${device.enabled ? "" : "off"}"></span><span class="summary-copy"><strong>${escapeHtml(device.name)}</strong><small>${device.profile === "h6005" ? "H6005" : "Classic (H6001)"} · ${escapeHtml(canonicalIdentifier(device.identifier))}</small></span><span class="disclosure">›</span></button>`;
    }
    return `
    <article class="editor-card expanded-editor" data-editor-key="${editorKey}" data-device-index="${index}">
      <div class="card-title collapsible-card-title" data-collapse-editor-header><label class="inline"><input type="checkbox" data-device-field="enabled" ${device.enabled ? "checked" : ""}><strong>${escapeHtml(device.name)}</strong></label><span><button class="remove-button" data-remove-device="${index}">Remove</button><button class="collapse-button" data-collapse-editor aria-label="Collapse light">›</button></span></div>
      <div class="field-grid">
        <label class="field">Name<input data-device-field="name" value="${escapeHtml(device.name)}"></label>
        <label class="field">Protocol<select data-device-field="profile"><option value="classic" ${device.profile === "classic" ? "selected" : ""}>Classic (H6001)</option><option value="h6005" ${device.profile === "h6005" ? "selected" : ""}>H6005</option></select></label>
        <label class="field full">Bluetooth identifier<input class="identifier-input" data-device-field="identifier" value="${escapeHtml(canonicalIdentifier(device.identifier))}" spellcheck="false"></label>
      </div>
    </article>`;
  }).join("") : '<div class="empty">No lights connected.</div>';

  $("#discovered-devices").innerHTML = discovered.map((device, index) => {
    const configuredDevice = configuredDeviceFor(device);
    const detail = configuredDevice
      ? `<small class="already-added">Already added as ${escapeHtml(configuredDevice.name)}</small>`
      : `<small class="muted">Detected ${protocolForDeviceName(device.name) === "h6005" ? "H6005" : "Classic (H6001)"} protocol</small>`;
    const action = configuredDevice
      ? '<button class="secondary compact" disabled>Added</button>'
      : `<button class="primary compact" data-add-discovered="${index}">Add</button>`;
    return `<article class="editor-card"><div class="card-title"><span><strong>${escapeHtml(device.name)}</strong>${detail}</span>${action}</div></article>`;
  }).join("");
}

function renderExperimentTargets() {
  const target = $("#experimental-target");
  target.innerHTML = settings.devices
    .filter((device) => device.enabled)
    .map((device) => `<option value="${escapeHtml(device.name)}">${escapeHtml(device.name)} · ${device.profile === "h6005" ? "H6005" : "H6001"}</option>`)
    .join("") || '<option value="">No enabled lights</option>';
  $("#experimental-test").disabled = !settings.devices.some((device) => device.enabled);
}

function renderAll() {
  updateHue(hueFromHex(settings.color), false);
  updateWhite(whitePositionFromHex(settings.white), false);
  $("#brightness").value = Math.round(settings.brightness * 100);
  $("#brightness-output").value = `${Math.round(settings.brightness * 100)}%`;
  $("#connection-hold-seconds").value = settings.connectionHoldSeconds;
  $("#breathing-pace").value = settings.breathingPaceSeconds;
  $("#breathing-pace-output").value = `${Number(settings.breathingPaceSeconds).toFixed(2)}s`;
  $("#breathing-hue-step").value = settings.breathingHueStepDegrees;
  $("#breathing-hue-step-output").value = `${Number(settings.breathingHueStepDegrees).toFixed(1)}°`;
  renderQuickPresets();
  renderPresets();
  renderSchedules();
  renderDevices();
  renderExperimentTargets();
  renderPrivilegedService();
}

function uniqueId() {
  return globalThis.crypto?.randomUUID?.() || `schedule-${Date.now()}`;
}

document.addEventListener("click", async (event) => {
  const collapseEditor = event.target.closest("[data-collapse-editor]");
  if (collapseEditor) {
    expandedEditorKey = null;
    renderPresets();
    renderDevices();
    return;
  }
  const collapseEditorHeader = event.target.closest("[data-collapse-editor-header]");
  const clickedHeaderControl = event.target.closest("button, input, select, textarea, a");
  if (collapseEditorHeader && !clickedHeaderControl) {
    event.preventDefault();
    expandedEditorKey = null;
    renderPresets();
    renderDevices();
    return;
  }
  const expandEditor = event.target.closest("[data-expand-editor]");
  if (expandEditor) {
    expandedEditorKey = expandEditor.dataset.expandEditor;
    renderPresets();
    renderDevices();
    return;
  }
  const clickedEditor = event.target.closest("[data-editor-key]");
  if (expandedEditorKey && clickedEditor?.dataset.editorKey !== expandedEditorKey) {
    expandedEditorKey = null;
    renderPresets();
    renderDevices();
  }

  const pageButton = event.target.closest("[data-page], [data-page-link]");
  if (pageButton) showPage(pageButton.dataset.page || pageButton.dataset.pageLink);

  const presetButton = event.target.closest("[data-preset]");
  if (presetButton) queueControl({ command: "preset", name: presetButton.dataset.preset, device: null });

  const powerButton = event.target.closest("[data-power]");
  if (powerButton) queueControl({ command: "power", on: powerButton.dataset.power === "true", device: null });

  const floodlightButton = event.target.closest("[data-floodlights]");
  if (floodlightButton) {
    const on = floodlightButton.dataset.floodlights === "true";
    setFloodlightsBusy(true);
    setStatus(`Turning floodlights ${on ? "on" : "off"}…`, "busy");
    try {
      setStatus(await call("set_floodlights", { on }));
    } catch (error) {
      setStatus(String(error), "error");
    } finally {
      setFloodlightsBusy(false);
    }
  }

  const removePreset = event.target.closest("[data-remove-preset]");
  if (removePreset) {
    expandedEditorKey = null;
    const [removed] = settings.presets.splice(Number(removePreset.dataset.removePreset), 1);
    settings.schedules = settings.schedules.filter((schedule) => schedule.preset !== removed.name);
    await save(); renderAll();
  }
  const removeSchedule = event.target.closest("[data-remove-schedule]");
  if (removeSchedule) {
    const index = Number(removeSchedule.dataset.removeSchedule);
    const schedule = settings.schedules[index];
    if (schedule.privilegedApprovedCommand) {
      setStatus("Revoking administrator approval…", "busy");
      try { await call("revoke_privileged_job", { id: schedule.id }); }
      catch (error) { setStatus(String(error), "error"); return; }
    }
    settings.schedules.splice(index, 1); await save(); renderSchedules();
  }
  const removeDevice = event.target.closest("[data-remove-device]");
  if (removeDevice) {
    expandedEditorKey = null;
    const [removed] = settings.devices.splice(Number(removeDevice.dataset.removeDevice), 1);
    settings.schedules.forEach((schedule) => { schedule.lights = schedule.lights.filter((name) => name !== removed.name); });
    await save(); renderAll();
  }

  const addDiscovered = event.target.closest("[data-add-discovered]");
  if (addDiscovered) {
    const device = discovered[Number(addDiscovered.dataset.addDiscovered)];
    const configuredDevice = configuredDeviceFor(device);
    if (configuredDevice) {
      setStatus(`Already added as ${configuredDevice.name}`);
      renderDevices();
      return;
    }
    settings.devices.push({ ...device, identifier: canonicalIdentifier(device.identifier), profile: protocolForDeviceName(device.name), enabled: true });
    expandedEditorKey = `device:${settings.devices.length - 1}`;
    await save(); renderAll();
  }

  const test = event.target.closest("[data-test-schedule]");
  if (test) {
    await save(); setStatus("Running test…", "busy");
    try { setStatus(await call("test_schedule", { id: test.dataset.testSchedule })); }
    catch (error) { setStatus(String(error), "error"); }
  }

  const approvePrivileged = event.target.closest("[data-approve-privileged]");
  if (approvePrivileged) {
    if (!privilegedService.installed || !privilegedService.healthy) {
      showPage("settings-page");
      setStatus(privilegedService.installed
        ? "Repair the privileged automation service first"
        : "Install the privileged automation service first", "error");
      return;
    }
    await save();
    setStatus("Waiting for administrator approval…", "busy");
    try {
      setStatus(await call("approve_privileged_job", { id: approvePrivileged.dataset.approvePrivileged }));
      settings = demoMode ? settings : await call("get_settings");
      if (demoMode) {
        const schedule = settings.schedules.find((item) => item.id === approvePrivileged.dataset.approvePrivileged);
        schedule.privilegedApprovedCommand = schedule.shellCommand.trim();
        schedule.privilegedApprovedAt = new Date().toISOString();
      }
      renderSchedules();
    } catch (error) { setStatus(String(error), "error"); }
  }

  const revokePrivileged = event.target.closest("[data-revoke-privileged]");
  if (revokePrivileged) {
    setStatus("Waiting for administrator approval…", "busy");
    try {
      setStatus(await call("revoke_privileged_job", { id: revokePrivileged.dataset.revokePrivileged }));
      const schedule = settings.schedules.find((item) => item.id === revokePrivileged.dataset.revokePrivileged);
      schedule.runAsAdministrator = false;
      schedule.privilegedApprovedCommand = "";
      schedule.privilegedApprovedAt = "";
      renderSchedules();
    } catch (error) { setStatus(String(error), "error"); }
  }
});

document.addEventListener("change", async (event) => {
  const presetCard = event.target.closest("[data-preset-index]");
  if (presetCard && (event.target.dataset.presetField || event.target.hasAttribute("data-preset-color"))) {
    const preset = settings.presets[Number(presetCard.dataset.presetIndex)];
    if (event.target.hasAttribute("data-preset-color")) {
      preset.value = preset.mode === "color"
        ? colorAtHue(Number(event.target.value))
        : whiteAtPosition(Number(event.target.value) / 100);
    } else {
      const field = event.target.dataset.presetField;
      const oldName = preset.name;
      preset[field] = field === "brightness" ? Number(event.target.value) / 100 : event.target.value;
      if (field === "name") settings.schedules.forEach((schedule) => { if (schedule.preset === oldName) schedule.preset = preset.name; });
    }
    await save(); renderAll();
  }
  const scheduleCard = event.target.closest("[data-schedule-index]");
  if (scheduleCard) {
    const schedule = settings.schedules[Number(scheduleCard.dataset.scheduleIndex)];
    if (event.target.dataset.scheduleField) {
      const field = event.target.dataset.scheduleField;
      schedule[field] = event.target.type === "checkbox" ? event.target.checked : event.target.value;
    }
    if (event.target.dataset.scheduleLight) {
      const light = event.target.dataset.scheduleLight;
      schedule.lights = event.target.checked ? [...new Set([...schedule.lights, light])] : schedule.lights.filter((name) => name !== light);
    }
    await save(); renderSchedules();
  }
  const deviceCard = event.target.closest("[data-device-index]");
  if (deviceCard && event.target.dataset.deviceField) {
    const device = settings.devices[Number(deviceCard.dataset.deviceIndex)];
    const oldName = device.name;
    const field = event.target.dataset.deviceField;
    device[field] = event.target.type === "checkbox"
      ? event.target.checked
      : field === "identifier" ? canonicalIdentifier(event.target.value) : event.target.value;
    if (field === "name") settings.schedules.forEach((schedule) => { schedule.lights = schedule.lights.map((name) => name === oldName ? device.name : name); });
    await save(); renderAll();
  }
});

makeDraggable($("#hue-track"), (position) => updateHue(position * 360));
makeDraggable($("#white-track"), updateWhite);
$("#hue-track").addEventListener("keydown", (event) => {
  if (!["ArrowLeft", "ArrowRight"].includes(event.key)) return;
  event.preventDefault();
  updateHue(hueFromHex(settings.color) + (event.key === "ArrowRight" ? 3 : -3));
  save();
});
$("#white-track").addEventListener("keydown", (event) => {
  if (!["ArrowLeft", "ArrowRight"].includes(event.key)) return;
  event.preventDefault();
  updateWhite(whitePositionFromHex(settings.white) + (event.key === "ArrowRight" ? 0.02 : -0.02));
  save();
});
$("#brightness").addEventListener("input", (event) => {
  settings.brightness = Number(event.target.value) / 100;
  $("#brightness-output").value = `${event.target.value}%`;
  queueControl({ command: "brightness", value: settings.brightness, device: null });
});
$("#brightness").addEventListener("change", save);

async function toggleEffect(effect) {
  const starting = activeEffect !== effect;
  setStatus(starting ? `Starting ${effect}…` : `Stopping ${effect}…`, "busy");
  try {
    if (starting && activeEffect) {
      await call("execute_control", { command: { command: "stop_effect" } });
      setEffectActive(null);
    }
    const message = await call("execute_control", {
      command: starting
        ? effect === "party"
          ? { command: "party", device: null }
          : {
              command: "breathe",
              pace_seconds: settings.breathingPaceSeconds,
              hue_step_degrees: settings.breathingHueStepDegrees,
              device: null,
            }
        : { command: "stop_effect" },
    });
    setEffectActive(starting ? effect : null);
    setStatus(message);
  } catch (error) {
    setEffectActive(null);
    setStatus(String(error), "error");
  }
}

$("#party-button").addEventListener("click", () => toggleEffect("party"));
$("#breathing-button").addEventListener("click", () => toggleEffect("breathe"));
$("#breathing-pace").addEventListener("input", (event) => {
  settings.breathingPaceSeconds = Number(event.target.value);
  $("#breathing-pace-output").value = `${settings.breathingPaceSeconds.toFixed(2)}s`;
});
$("#breathing-pace").addEventListener("change", async () => {
  await save();
  if (activeEffect === "breathe") {
    await call("execute_control", { command: { command: "stop_effect" } });
    setEffectActive(null);
    await toggleEffect("breathe");
  }
});
$("#breathing-hue-step").addEventListener("input", (event) => {
  settings.breathingHueStepDegrees = Number(event.target.value);
  $("#breathing-hue-step-output").value = `${settings.breathingHueStepDegrees.toFixed(1)}°`;
});
$("#breathing-hue-step").addEventListener("change", async () => {
  await save();
  if (activeEffect === "breathe") {
    await call("execute_control", { command: { command: "stop_effect" } });
    setEffectActive(null);
    await toggleEffect("breathe");
  }
});

$("#experimental-candidate").addEventListener("change", (event) => {
  $("#experimental-payload").value = event.target.value;
});
$("#experimental-test").addEventListener("click", () => {
  const device = $("#experimental-target").value;
  if (!device) return setStatus("Choose an enabled light", "error");
  queueControl({ command: "experiment", payload: $("#experimental-payload").value.trim(), device });
});
$("#experimental-restore").addEventListener("click", () => {
  const device = $("#experimental-target").value || null;
  queueControl({ command: "color", value: settings.color, brightness: settings.brightness, device });
});

$("#add-preset").addEventListener("click", async () => {
  settings.presets.push({ name: `preset-${settings.presets.length + 1}`, mode: "color", value: "#8e72e8", brightness: 0.5 });
  expandedEditorKey = `preset:${settings.presets.length - 1}`;
  await save(); renderAll();
});
$("#add-schedule").addEventListener("click", async () => {
  if (!settings.presets.length) { showPage("settings-page"); return setStatus("Add a preset first", "error"); }
  settings.schedules.push({ id: uniqueId(), name: "New automation", time: "20:00", enabled: true, lights: [], preset: settings.presets[0].name, floodlights: "unchanged", shellCommand: "", runAsAdministrator: false, privilegedApprovedCommand: "", privilegedApprovedAt: "" });
  await save(); renderSchedules();
});
$("#discover-button").addEventListener("click", async () => {
  setStatus("Scanning for Govee lights…", "busy");
  setDiscoveryBusy(true);
  try {
    discovered = (await call("discover_lights")).map((device) => ({
      ...device,
      identifier: canonicalIdentifier(device.identifier),
    }));
    const existingCount = discovered.filter(configuredDeviceFor).length;
    renderDevices();
    setStatus(discovered.length
      ? `Found ${discovered.length} light(s) · ${existingCount} already added`
      : "No Govee lights found");
  } catch (error) {
    renderDevices();
    setStatus(String(error), "error");
  } finally {
    setDiscoveryBusy(false);
  }
});
$("#connection-hold-seconds").addEventListener("change", async (event) => {
  settings.connectionHoldSeconds = Math.max(1, Math.min(60, Math.round(Number(event.target.value) || 6)));
  event.target.value = settings.connectionHoldSeconds;
  await save();
});
$("#privileged-service-install").addEventListener("click", async () => {
  setStatus("Waiting for administrator approval…", "busy");
  try {
    setStatus(await call("install_privileged_service"));
    await refreshPrivilegedService();
  } catch (error) { setStatus(String(error), "error"); }
});
$("#privileged-service-remove").addEventListener("click", async () => {
  setStatus("Waiting for administrator approval…", "busy");
  try {
    setStatus(await call("uninstall_privileged_service"));
    settings.schedules.forEach((schedule) => {
      schedule.runAsAdministrator = false;
      schedule.privilegedApprovedCommand = "";
      schedule.privilegedApprovedAt = "";
    });
    await refreshPrivilegedService();
    renderSchedules();
  } catch (error) { setStatus(String(error), "error"); }
});
$("#close-button").addEventListener("click", () => call("hide_window"));
$("#quit-button").addEventListener("click", () => call("quit_app"));

document.addEventListener("keydown", (event) => {
  const shortcuts = {
    "1": "controller-page",
    "2": "automations-page",
    "3": "settings-page",
  };
  if (event.metaKey && shortcuts[event.key]) {
    event.preventDefault();
    showPage(shortcuts[event.key]);
  } else if (event.key === "Escape") {
    event.preventDefault();
    call("hide_window");
  }
});

settings = demoMode ? structuredClone(demoSettings) : await call("get_settings");
await refreshPrivilegedService();
renderAll();
const requestedPage = new URLSearchParams(location.search).get("page");
if (requestedPage === "presets") showPage("settings-page");
else if (["automations", "settings"].includes(requestedPage)) showPage(`${requestedPage}-page`);
