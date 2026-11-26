// Which enabled lights the Control page drives. Stored as the lights turned
// off, so new lights start selected.
const STORAGE = "grindlewald.deselectedLights";

export function loadDeselected(storage = globalThis.localStorage) {
  try {
    const names = JSON.parse(storage?.getItem(STORAGE) ?? "[]");
    return new Set(Array.isArray(names) ? names : []);
  } catch {
    return new Set();
  }
}

export function storeDeselected(deselected, storage = globalThis.localStorage) {
  try {
    storage?.setItem(STORAGE, JSON.stringify([...deselected]));
  } catch {
    // Selection still works for this session.
  }
}

const enabledLights = (devices) => devices.filter((device) => device.enabled);

// The devices list as the Control page sees it: deselected lights count as off.
export const controlledDevices = (devices, deselected) =>
  devices.map((device) => ({ ...device, enabled: device.enabled && !deselected.has(device.name) }));

// null targets every enabled light, so all-selected keeps meaning "all".
export function controlTarget(devices, deselected) {
  const enabled = enabledLights(devices);
  if (!enabled.some((device) => deselected.has(device.name))) return null;
  return enabled.filter((device) => !deselected.has(device.name)).map((device) => device.name);
}

// Commands addressed to every light (device: null) go to the selection instead.
export function withControlTarget(command, target) {
  return command && "device" in command && command.device === null ? { ...command, device: target } : command;
}

export function renameDeselected(deselected, oldName, newName) {
  if (!deselected.delete(oldName)) return false;
  deselected.add(newName);
  return true;
}

export function lightPickerHtml(devices, deselected, escapeHtml) {
  const enabled = enabledLights(devices);
  if (enabled.length < 2) return "";
  return enabled.map(({ name }) =>
    `<button class="light-toggle" data-light="${escapeHtml(name)}" aria-pressed="${!deselected.has(name)}">${escapeHtml(name)}</button>`).join("");
}

// Press a light to flip it, then drag across others to give them the same state.
export function bindLightPicker(container, onChange) {
  let paintState = null;
  // Set by a pointer press so its follow-up click isn't treated as a keyboard press.
  // (Some browsers report touch clicks with detail 0, like keyboard ones.)
  let pointerPressed = false;
  const toggleAt = (x, y) => document.elementFromPoint(x, y)?.closest("[data-light]");
  const paint = (toggle) => {
    if (!toggle || !container.contains(toggle) || toggle.getAttribute("aria-pressed") === String(paintState)) return;
    toggle.setAttribute("aria-pressed", String(paintState));
    onChange(toggle.dataset.light, paintState);
  };
  container.addEventListener("pointerdown", (event) => {
    const toggle = event.target.closest("[data-light]");
    if (!toggle || event.button !== 0) return;
    event.preventDefault();
    pointerPressed = true;
    paintState = toggle.getAttribute("aria-pressed") !== "true";
    container.setPointerCapture(event.pointerId);
    paint(toggle);
  });
  container.addEventListener("pointermove", (event) => {
    if (paintState !== null) paint(toggleAt(event.clientX, event.clientY));
  });
  for (const type of ["pointerup", "pointercancel", "lostpointercapture"]) {
    container.addEventListener(type, () => { paintState = null; });
  }
  container.addEventListener("keydown", () => { pointerPressed = false; });
  // Keyboard presses arrive as clicks with no pointer behind them.
  container.addEventListener("click", (event) => {
    const toggle = event.target.closest("[data-light]");
    if (pointerPressed) { pointerPressed = false; return; }
    if (!toggle || event.detail !== 0) return;
    paintState = toggle.getAttribute("aria-pressed") !== "true";
    paint(toggle);
    paintState = null;
  });
}
