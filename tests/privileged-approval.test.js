import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const source = readFileSync(new URL("../src/app.js", import.meta.url), "utf8");
const approvalHandler = source.slice(
  source.indexOf('  const approvePrivileged = event.target.closest'),
  source.indexOf('  const revokePrivileged = event.target.closest'),
);

function harness(save = async () => true) {
  const approval = Promise.withResolvers();
  const buttons = [{ dataset: { approvePrivileged: "first" } }, { dataset: { approvePrivileged: "second" } }];
  const calls = [];
  const context = vm.createContext({
    approvingPrivilegedJob: false,
    privilegedService: { installed: true, healthy: true },
    document: { querySelectorAll: () => buttons },
    save,
    setStatus() {},
    renderSchedules() {},
    demoMode: false,
    settings: {},
    call: async (command, args) => {
      calls.push({ command, args });
      return command === "approve_privileged_job" ? approval.promise : {};
    },
  });
  const click = vm.runInContext(`(async (event) => { ${approvalHandler} })`, context);
  return { approval, buttons, calls, click: (button = buttons[0]) => click({ target: { closest: () => button } }) };
}

test("rapid clicks during saving and authorization open only one approval prompt", async () => {
  const saving = Promise.withResolvers();
  const h = harness(() => saving.promise);
  const first = h.click();
  assert.ok(h.buttons.every((button) => button.disabled));
  await h.click();
  await h.click(h.buttons[1]);
  assert.equal(h.calls.length, 0);
  saving.resolve(true);
  await Promise.resolve();
  await h.click();
  await h.click(h.buttons[1]);
  assert.equal(h.calls.filter(({ command }) => command === "approve_privileged_job").length, 1);
  h.approval.resolve("Approved");
  await first;
  assert.ok(h.buttons.every((button) => !button.disabled));
});

test("cancelled authorization unlocks newly rendered buttons and permits retry", async () => {
  const h = harness();
  const first = h.click();
  await Promise.resolve();
  h.buttons.splice(0, 1, { dataset: { approvePrivileged: "first" }, disabled: true });
  await h.click();
  h.approval.reject(new Error("Cancelled"));
  await first;
  assert.ok(h.buttons.every((button) => !button.disabled));
  await h.click();
  assert.equal(h.calls.filter(({ command }) => command === "approve_privileged_job").length, 2);
});

test("failed saving does not open authorization and allows another attempt", async () => {
  const h = harness(async () => false);
  await h.click();
  assert.equal(h.calls.length, 0);
  assert.ok(h.buttons.every((button) => !button.disabled));
});
