// One-click Shortcuts page buttons. Root ones run through the privileged helper
// once approved; one macOS prompt approves every pending command together.
export const needsApproval = (shortcut) => Boolean(shortcut.runAsAdministrator)
  && Boolean(shortcut.command.trim())
  && shortcut.privilegedApprovedCommand !== shortcut.command.trim();

export const pendingApprovals = (shortcuts = []) => shortcuts.filter(needsApproval);

export const newShortcut = (id) => ({ id, name: "New shortcut", command: "", runAsAdministrator: false, privilegedApprovedCommand: "" });
