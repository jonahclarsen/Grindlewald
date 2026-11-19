import test from "node:test";
import assert from "node:assert/strict";
import { controlTarget, controlledDevices, lightPickerHtml, loadDeselected, renameDeselected, storeDeselected, withControlTarget } from "../src/light-picker.js";

const devices = [
  { name: "Desk", profile: "h6005", enabled: true },
  { name: "Shelf", profile: "classic", enabled: true },
  { name: "Attic", profile: "classic", enabled: false },
];
const escape = (value) => value;

test("all enabled lights selected targets every light", () => {
  assert.equal(controlTarget(devices, new Set()), null);
  // A disabled light being deselected does not narrow the target.
  assert.equal(controlTarget(devices, new Set(["Attic"])), null);
});

test("a partial selection targets only the selected enabled lights", () => {
  assert.deepEqual(controlTarget(devices, new Set(["Shelf"])), ["Desk"]);
  assert.deepEqual(controlTarget(devices, new Set(["Desk", "Shelf"])), []);
});

test("only commands addressed to every light follow the selection", () => {
  assert.deepEqual(withControlTarget({ command: "power", on: true, device: null }, ["Desk"]), { command: "power", on: true, device: ["Desk"] });
  assert.deepEqual(withControlTarget({ command: "experiment", device: "Shelf" }, ["Desk"]), { command: "experiment", device: "Shelf" });
  assert.deepEqual(withControlTarget({ command: "stop_effect" }, ["Desk"]), { command: "stop_effect" });
  assert.deepEqual(withControlTarget({ command: "power", device: null }, null), { command: "power", device: null });
});

test("deselected lights count as off for model-specific controls", () => {
  assert.deepEqual(controlledDevices(devices, new Set(["Desk"])).map((device) => device.enabled), [false, true, false]);
});

test("the picker shows enabled lights only when there is a choice", () => {
  assert.equal(lightPickerHtml(devices.slice(0, 1), new Set(), escape), "");
  const html = lightPickerHtml(devices, new Set(["Shelf"]), escape);
  assert.match(html, /data-light="Desk" aria-pressed="true"/);
  assert.match(html, /data-light="Shelf" aria-pressed="false"/);
  assert.doesNotMatch(html, /Attic/);
});

test("selection survives renames and storage failures", () => {
  const deselected = new Set(["Shelf"]);
  assert.equal(renameDeselected(deselected, "Shelf", "Bookshelf"), true);
  assert.deepEqual([...deselected], ["Bookshelf"]);
  assert.equal(renameDeselected(deselected, "Desk", "Table"), false);

  const values = new Map();
  const storage = { getItem: (key) => values.get(key) ?? null, setItem: (key, value) => values.set(key, value) };
  storeDeselected(deselected, storage);
  assert.deepEqual([...loadDeselected(storage)], ["Bookshelf"]);
  const broken = { getItem() { throw Error("blocked"); }, setItem() { throw Error("blocked"); } };
  assert.deepEqual([...loadDeselected(broken)], []);
  storeDeselected(deselected, broken);
});
