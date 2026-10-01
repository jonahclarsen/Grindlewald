import test from "node:test";
import assert from "node:assert/strict";
import { createAutomationEditorState } from "../src/automation-editors.js";

test("automations start collapsed and remain open while active", () => {
  let time = 0;
  const state = createAutomationEditorState(() => time);
  assert.equal(state.expandedIds.size, 0);
  state.setActive(true);
  state.expandedIds.add("one");
  time = 120_000;
  assert.equal(state.setActive(true), false);
  assert.deepEqual([...state.expandedIds], ["one"]);
});

test("returning before one minute preserves open automations and resets the grace period", () => {
  let time = 0;
  const state = createAutomationEditorState(() => time);
  state.expandedIds.add("one");
  state.setActive(false);
  time = 59_999;
  assert.equal(state.setActive(true), false);
  assert.deepEqual([...state.expandedIds], ["one"]);
  state.setActive(false);
  time += 59_999;
  assert.equal(state.setActive(true), false);
  assert.deepEqual([...state.expandedIds], ["one"]);
});

test("page changes and window events do not restart an existing absence", () => {
  let time = 0;
  const state = createAutomationEditorState(() => time);
  state.expandedIds.add("one");
  state.expandedIds.add("two");
  state.setActive(false);
  time = 30_000;
  state.setActive(false);
  time = 60_000;
  assert.equal(state.setActive(true), true);
  assert.equal(state.expandedIds.size, 0);
  assert.equal(state.setActive(true), false);
});
