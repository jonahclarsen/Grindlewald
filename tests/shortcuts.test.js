import assert from "node:assert/strict";
import test from "node:test";
import { needsApproval, newShortcut, pendingApprovals } from "../src/shortcuts.js";

test("only changed root commands need approval", () => {
  const shortcut = { ...newShortcut("a"), command: "pmset -a lowpowermode 1 ", runAsAdministrator: true };
  assert.equal(needsApproval(shortcut), true);
  shortcut.privilegedApprovedCommand = "pmset -a lowpowermode 1";
  assert.equal(needsApproval(shortcut), false);
  shortcut.command = "pmset -a lowpowermode 0";
  assert.equal(needsApproval(shortcut), true);
  assert.equal(needsApproval({ ...shortcut, runAsAdministrator: false }), false);
  assert.equal(needsApproval({ ...shortcut, command: "  " }), false);
  assert.deepEqual(pendingApprovals([shortcut, newShortcut("b")]), [shortcut]);
  assert.deepEqual(pendingApprovals(undefined), []);
});
