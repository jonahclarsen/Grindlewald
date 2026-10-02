import test from "node:test";
import assert from "node:assert/strict";
import { kelvinAtPosition, whiteAtPosition, whitePlaybackPosition, whiteSeekPhase, whiteBreathingCommand } from "../src/white-breathing.js";

test("white playback reflects at warm and cool instead of wrapping abruptly", () => {
  assert.equal(whitePlaybackPosition(0), 0);
  assert.equal(whitePlaybackPosition(765), 1);
  assert.equal(whitePlaybackPosition(1530), 0);
  for (let phase = 1; phase < 765; phase++) {
    assert.equal(whitePlaybackPosition(phase), whitePlaybackPosition(1530 - phase));
  }
  assert.equal(kelvinAtPosition(0), 2000);
  assert.equal(kelvinAtPosition(1), 9000);
  assert.equal(whiteAtPosition(0), "#ff8d0b");
  assert.equal(whiteAtPosition(0.5), "#ffeede");
  assert.equal(whiteAtPosition(1), "#d9e1ff");
});

test("dragging white playback retains its current sweep direction", () => {
  assert.equal(whiteSeekPhase(0.4, 100), 306);
  assert.equal(whiteSeekPhase(0.4, 1000), 1224);
  assert.equal(whitePlaybackPosition(whiteSeekPhase(0.4, 1000)), 0.4);
});

test("white pace controls send a one-way sweep duration without color step settings", () => {
  for (const seconds of [5, 30, 120]) {
    assert.deepEqual(whiteBreathingCommand(seconds), { command: "breathe_white", sweep_seconds: seconds, device: null });
  }
  for (const invalid of [0, 6, 125, NaN, 30.5]) assert.throws(() => whiteBreathingCommand(invalid));
});
