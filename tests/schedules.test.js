import assert from "node:assert/strict";
import test from "node:test";
import { disableScheduleFor, isScheduleEnabled, schedulePauseLabel } from "../src/schedules.js";

test("every duration pauses for exactly N × 24 hours and resumes at the deadline", () => {
  // Crosses the autumn daylight-saving transition in America/Vancouver.
  const now = Date.parse("2026-10-31T12:00:00-07:00");
  for (let days = 1; days <= 8; days++) {
    const schedule = { enabled: true };
    disableScheduleFor(schedule, days, now);
    const restored = JSON.parse(JSON.stringify(schedule));
    assert.equal(restored.disabledUntil - now, days * 86_400_000);
    assert.equal(isScheduleEnabled(restored, restored.disabledUntil - 1), false);
    assert.equal(isScheduleEnabled(restored, restored.disabledUntil), true);
    assert.match(schedulePauseLabel(restored, now), /^Disabled until /);
    assert.equal(schedulePauseLabel(restored, restored.disabledUntil), "");
  }
});

test("disabled and already paused automations cannot be paused", () => {
  assert.throws(() => disableScheduleFor({ enabled: false }, 1, 0), /Only enabled/);
  assert.throws(() => disableScheduleFor({ enabled: true, disabledUntil: 100 }, 1, 0), /Only enabled/);
  assert.equal(isScheduleEnabled({ enabled: false, disabledUntil: 100 }, 200), false);
  assert.equal(schedulePauseLabel({ enabled: false }, 0), "Disabled");
});

test("rejects invalid durations without changing the automation", () => {
  for (const days of [0, 9, -1, 1.5, NaN, Infinity, "2"]) {
    const schedule = { enabled: true };
    assert.throws(() => disableScheduleFor(schedule, days, 0), /between 1 and 8/);
    assert.deepEqual(schedule, { enabled: true });
  }
});
