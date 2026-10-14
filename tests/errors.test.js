import assert from "node:assert/strict";
import test from "node:test";
import { createStatusReporter } from "../src/errors.js";

function reporter() {
  const rendered = [];
  const recorded = [];
  const pending = new Map();
  let next = 0;
  const status = createStatusReporter({
    render: (message, kind) => rendered.push({ message, kind }),
    recordError: (message) => recorded.push(message),
    schedule: (callback) => { pending.set(++next, callback); return next; },
    cancel: (id) => pending.delete(id),
  });
  const flush = () => {
    const callbacks = [...pending.values()];
    pending.clear();
    callbacks.forEach((callback) => callback());
  };
  return { status, rendered, recorded, flush };
}

test("missing-light errors stay steady through retries and background saves", () => {
  const { status, rendered, recorded, flush } = reporter();
  const error = "could not find Test lamp over Bluetooth";
  status("Connecting…", "busy");
  status(error, "error");
  flush();
  status("Saved");
  status("Connecting…", "busy");
  flush();
  assert.deepEqual(rendered, [{ message: error, kind: "error" }]);
  assert.deepEqual(recorded, [error]);
});

test("success replaces the failure even when another retry starts before it renders", () => {
  const { status, rendered, flush } = reporter();
  status("Light unavailable", "error");
  status("Updated 1 light(s)");
  status("Connecting…", "busy");
  flush();
  assert.deepEqual(rendered.at(-1), { message: "Updated 1 light(s)", kind: "ready" });
  status("Connecting…", "busy");
  flush();
  assert.deepEqual(rendered.at(-1), { message: "Connecting…", kind: "busy" });
});

test("a new failure cancels a pending success and records the full error", () => {
  const { status, rendered, recorded, flush } = reporter();
  status("Updated 1 light(s)");
  status("Operation failed\ncould not find Test lamp over Bluetooth", "error");
  flush();
  assert.deepEqual(rendered, [{ message: "could not find Test lamp over Bluetooth", kind: "error" }]);
  assert.deepEqual(recorded, ["Operation failed\ncould not find Test lamp over Bluetooth"]);
});
