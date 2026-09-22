import assert from "node:assert/strict";
import test from "node:test";
import { minimumCycleSeconds, formatCycleSeconds } from "../src/breathing.js";

test("duration bounds allow every RGB step without exceeding the update rate", () => {
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
