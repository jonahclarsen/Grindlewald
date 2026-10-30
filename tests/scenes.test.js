import assert from "node:assert/strict";
import test from "node:test";
import { availableScenes, diyBreatheAvailable, diyBreatheCommand, sceneCommand } from "../src/scenes.js";

const light = (profile, enabled = true) => ({ name: profile, identifier: profile, profile, enabled });

test("scenes come from the enabled lights' models", () => {
  assert.deepEqual(availableScenes([light("classic")]).map(({ id }) => id), ["breathe", "reading", "romantic", "relax", "vivid", "colorful", "morning", "sunset"]);
  assert.deepEqual(availableScenes([light("h6005"), light("classic", false)]).map(({ id }) => id), ["breathe", "reading", "romantic", "candlelight", "fade", "energetic", "heartbeat", "blinking"]);
  assert.equal(availableScenes([light("classic"), light("h6005")]).length, 13);
  assert.deepEqual(availableScenes([light("h6005", false)]), []);
});

test("scenes only some enabled lights offer are marked partial", () => {
  const mixed = Object.fromEntries(availableScenes([light("classic"), light("h6005")]).map((scene) => [scene.id, scene]));
  assert.equal(mixed.breathe.partial, false);
  assert.equal(mixed.sunset.partial, true);
  assert.deepEqual(mixed.heartbeat.models, ["h6005"]);
  assert.equal(availableScenes([light("classic")]).some(({ partial }) => partial), false);
});

test("DIY breathing needs an enabled H6005 and breathes the selected color", () => {
  assert.equal(diyBreatheAvailable([light("classic"), light("h6005", false)]), false);
  assert.equal(diyBreatheAvailable([light("classic"), light("h6005")]), true);
  assert.deepEqual(diyBreatheCommand("#ff4f22", 50), { command: "diy_breathe", colors: ["#ff4f22"], speed: 50, device: null });
  assert.deepEqual(sceneCommand("breathe"), { command: "scene", scene: "breathe", device: null });
});
