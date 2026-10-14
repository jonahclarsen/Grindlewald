import test from "node:test";
import assert from "node:assert/strict";
import { lightsToMeasure, measureStepIntervalCommand } from "../src/step-interval.js";

test("only enabled H6001 (Classic) lights need their step interval measured", () => {
  const h6001 = { name: "Desk", profile: "classic", enabled: true };
  const h6005 = { name: "Room", profile: "h6005", enabled: true };
  assert.deepEqual(lightsToMeasure([h6005]), []);
  assert.deepEqual(lightsToMeasure([h6001, h6005]), [h6001]);
  assert.deepEqual(lightsToMeasure([{ ...h6001, enabled: false }]), []);
});

test("measuring targets one named light", () => {
  assert.deepEqual(measureStepIntervalCommand({ name: "Desk" }), { command: "measure_step_interval", device: "Desk" });
});
