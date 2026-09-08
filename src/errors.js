export function summarizeError(message) {
  if (/floodlight/i.test(message) && /bad file descriptor/i.test(message)) {
    return "Floodlights couldn’t connect to TP-Link. Check whether your firewall (such as Little Snitch) is blocking Grindlewald or Python, then try again.";
  }
  if (/floodlight/i.test(message) && /ConnectionError|timed?\s*out|NameResolutionError/i.test(message)) {
    return "Floodlights couldn’t reach TP-Link. Check your internet connection and firewall, then try again.";
  }
  const lastLine = String(message).trim().split(/\r?\n/).filter((line) => line.trim()).at(-1)?.trim();
  const summary = lastLine || "Something went wrong. Copy the error details for help.";
  return summary.length > 180 ? `${summary.slice(0, 179)}…` : summary;
}

export function createErrorPanel(invoke) {
  const panel = document.querySelector("#error-panel");
  const details = document.querySelector("#error-details");
  const toggle = document.querySelector("#error-toggle");
  const copy = document.querySelector("#error-copy");
  const copyLabel = document.querySelector("#error-copy-label");
  let fullMessage = "";
  let revision = 0;

  const scrollToEnd = () => requestAnimationFrame(() => {
    details.scrollTop = details.scrollHeight;
  });

  toggle.addEventListener("click", () => {
    const expanded = toggle.getAttribute("aria-expanded") !== "true";
    toggle.setAttribute("aria-expanded", String(expanded));
    document.querySelector("#error-toggle-label").textContent = expanded ? "Less detail" : "Show details";
    panel.classList.toggle("expanded", expanded);
    scrollToEnd();
  });

  document.querySelector("#error-dismiss").addEventListener("click", () => {
    panel.hidden = true;
    revision += 1;
  });

  copy.addEventListener("click", async () => {
    const copiedRevision = revision;
    copy.disabled = true;
    try {
      if (invoke) await invoke("copy_error_details", { text: fullMessage });
      else await navigator.clipboard.writeText(fullMessage);
      if (revision === copiedRevision) copyLabel.textContent = "Copied";
    } catch {
      if (revision === copiedRevision) {
        copyLabel.textContent = "Select & ⌘C";
        const selection = window.getSelection();
        const range = document.createRange();
        range.selectNodeContents(details);
        selection.removeAllRanges();
        selection.addRange(range);
        details.focus();
      }
    } finally {
      if (revision === copiedRevision) copy.disabled = false;
    }
  });

  return (message) => {
    revision += 1;
    fullMessage = String(message);
    document.querySelector("#error-summary").textContent = summarizeError(fullMessage);
    details.textContent = fullMessage;
    copyLabel.textContent = "Copy error";
    copy.disabled = false;
    panel.classList.remove("expanded");
    toggle.setAttribute("aria-expanded", "false");
    document.querySelector("#error-toggle-label").textContent = "Show details";
    panel.hidden = false;
    scrollToEnd();
  };
}
