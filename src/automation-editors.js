export function createAutomationEditorState(now = Date.now) {
  const expandedIds = new Set();
  let awaySince = null;

  function setActive(active) {
    if (!active) {
      awaySince ??= now();
      return false;
    }
    const expired = awaySince !== null && now() - awaySince >= 60_000;
    awaySince = null;
    if (expired) expandedIds.clear();
    return expired;
  }

  return { expandedIds, setActive };
}
