import assert from "node:assert/strict";
import test from "node:test";
import { createControlQueue } from "../src/controls.js";

const color = (value, brightness = 0.4) => ({ command: "color", value, brightness, device: null });
const brightness = (value) => ({ command: "brightness", value, device: null });

function disconnectedQueue(callbacks) {
  const connection = Promise.withResolvers();
  const sent = [];
  const queue = createControlQueue(async (command) => {
    sent.push(command);
    if (sent.length === 1) await connection.promise;
    return "Updated";
  }, callbacks);
  return { queue, sent, connection };
}

test("reconnect applies the latest color and brightness after dragging both sliders", async () => {
  for (const mode of ["color", "white"]) {
    const { queue, sent, connection } = disconnectedQueue();
    const first = { ...color("#ff0000"), command: mode, ...(mode === "white" ? { kelvin: 2700 } : {}) };
    const done = queue.enqueue(first);
    queue.enqueue({ ...first, value: "#ffaa00" });
    queue.enqueue({ ...first, value: "#ffcc00" });
    queue.enqueue(brightness(0.7));
    queue.enqueue(brightness(0));
    assert.deepEqual(sent, [first]);
    connection.resolve();
    await done;
    assert.deepEqual(sent, [first, { ...first, value: "#ffcc00", brightness: 0 }]);
  }
});

test("a single color change followed by brightness survives reconnect", async () => {
  const { queue, sent, connection } = disconnectedQueue();
  const done = queue.enqueue(color("#ff0000"));
  queue.enqueue(brightness(0.8));
  connection.resolve();
  await done;
  assert.deepEqual(sent, [color("#ff0000"), brightness(0.8)]);
});

test("brightness followed by color applies the latest combined values", async () => {
  const { queue, sent, connection } = disconnectedQueue();
  const done = queue.enqueue(brightness(0.1));
  queue.enqueue(brightness(0.6));
  queue.enqueue(color("#00ff00", 0.6));
  connection.resolve();
  await done;
  assert.deepEqual(sent, [brightness(0.1), color("#00ff00", 0.6)]);
});

test("different targets and intervening commands keep their order", async () => {
  const { queue, sent, connection } = disconnectedQueue();
  const commands = [brightness(0.1), color("#ff0000"), { ...brightness(0.8), device: "Test lamp" },
    { command: "power", on: false, device: null }, brightness(0.3)];
  const done = queue.enqueue(commands[0]);
  commands.slice(1).forEach((command) => queue.enqueue(command));
  connection.resolve();
  await done;
  assert.deepEqual(sent, commands);
});

test("disconnect clears pending changes and suppresses stale completion", async () => {
  const statuses = [];
  const { queue, sent, connection } = disconnectedQueue({ onSuccess: (message) => statuses.push(message) });
  const done = queue.enqueue(color("#ff0000"));
  queue.enqueue(brightness(0.8));
  queue.clear();
  connection.resolve();
  await done;
  assert.equal(sent.length, 1);
  assert.deepEqual(statuses, []);
  await queue.enqueue(brightness(0.2));
  assert.deepEqual(sent.at(-1), brightness(0.2));
  assert.deepEqual(statuses, ["Updated"]);
});

test("a failed connection still drains pending changes and permits later updates", async () => {
  const errors = [];
  const { queue, sent, connection } = disconnectedQueue({ onError: (error) => errors.push(error.message) });
  const done = queue.enqueue(color("#ff0000"));
  queue.enqueue(brightness(0.8));
  connection.reject(new Error("Connection failed"));
  await done;
  await queue.enqueue(brightness(0.3));
  assert.deepEqual(errors, ["Connection failed"]);
  assert.deepEqual(sent, [color("#ff0000"), brightness(0.8), brightness(0.3)]);
});
