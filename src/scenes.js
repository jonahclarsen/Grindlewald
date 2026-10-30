// Built-in scenes and the models that offer them; keep in sync with Scene::code
// in src-tauri/src/protocol.rs.
const scenes = [
  ["breathe", "Breathe", ["classic", "h6005"]],
  ["reading", "Reading", ["classic", "h6005"]],
  ["romantic", "Romantic", ["classic", "h6005"]],
  ["relax", "Relax", ["classic"]],
  ["vivid", "Vivid", ["classic"]],
  ["colorful", "Colorful", ["classic"]],
  ["morning", "Morning", ["classic"]],
  ["sunset", "Sunset", ["classic"]],
  ["candlelight", "Candlelight", ["h6005"]],
  ["fade", "Fade", ["h6005"]],
  ["energetic", "Energetic", ["h6005"]],
  ["heartbeat", "Heartbeat", ["h6005"]],
  ["blinking", "Blinking", ["h6005"]],
];

const enabledProfiles = (devices) => new Set(devices.filter((device) => device.enabled).map((device) => device.profile));

// Scenes at least one enabled light offers; the others are left unchanged.
export function availableScenes(devices) {
  const profiles = enabledProfiles(devices);
  return scenes
    .filter(([, , models]) => models.some((model) => profiles.has(model)))
    .map(([id, label, models]) => ({ id, label, models: models.filter((model) => profiles.has(model)), partial: [...profiles].some((profile) => !models.includes(profile)) }));
}

export const diyBreatheAvailable = (devices) => enabledProfiles(devices).has("h6005");

export const sceneCommand = (scene) => ({ command: "scene", scene, device: null });

export const diyBreatheCommand = (color, speed) => ({ command: "diy_breathe", colors: [color], speed, device: null });
