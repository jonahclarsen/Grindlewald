import assert from "node:assert/strict";
import test from "node:test";
import { minimumCycleSeconds, formatCycleSeconds, cycleSecondsAfterStepChange } from "../src/breathing.js";

test("duration bounds preserve the existing minimum average pace", () => {
  assert.equal(minimumCycleSeconds(1), 459);
  assert.equal(minimumCycleSeconds(10), 46);
  assert.equal(minimumCycleSeconds(100), 5);
  for (let step = 1; step <= 100; step++) {
    const minimum = minimumCycleSeconds(step);
    const updates = Math.ceil(1530 / step);
    assert(minimum / updates >= 0.3);
    assert((minimum - 1) / updates < 0.3);
  }
});

test("cycle duration is readable in seconds and minutes", () => {
  assert.equal(formatCycleSeconds(5), "5s");
  assert.equal(formatCycleSeconds(459), "7m 39s");
  assert.equal(formatCycleSeconds(600), "10m 0s");
  assert.equal(formatCycleSeconds(3600), "60m 0s");
});

test("minimum cycle time follows changes to color step in both directions", () => {
  assert.equal(cycleSecondsAfterStepChange(459, 1, 2), 230);
  assert.equal(cycleSecondsAfterStepChange(230, 2, 100), 5);
  assert.equal(cycleSecondsAfterStepChange(5, 100, 1), 459);
  assert.equal(cycleSecondsAfterStepChange(600, 1, 100), 600);
  assert.equal(cycleSecondsAfterStepChange(60, 100, 1), 459);
  assert.equal(cycleSecondsAfterStepChange(46, 10, 11), minimumCycleSeconds(11));
});
