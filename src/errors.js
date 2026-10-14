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
  const open = document.querySelector("#error-open");
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

  const close = () => {
    panel.hidden = true;
    open.setAttribute("aria-expanded", "false");
  };
  open.addEventListener("click", () => {
    panel.hidden = !panel.hidden;
    open.setAttribute("aria-expanded", String(!panel.hidden));
    if (!panel.hidden) scrollToEnd();
  });
  panel.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close();
      open.focus();
    }
  });

  toggle.addEventListener("click", () => {
    const expanded = toggle.getAttribute("aria-expanded") !== "true";
    toggle.setAttribute("aria-expanded", String(expanded));
    document.querySelector("#error-toggle-label").textContent = expanded ? "Less detail" : "Show details";
    panel.classList.toggle("expanded", expanded);
    scrollToEnd();
  });

  document.querySelector("#error-dismiss").addEventListener("click", () => {
    close();
    open.focus();
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
    if (fullMessage === String(message)) return;
    revision += 1;
    fullMessage = String(message);
    document.querySelector("#error-summary").textContent = summarizeError(fullMessage);
    details.textContent = fullMessage;
    copyLabel.textContent = "Copy error";
    copy.disabled = false;
    open.hidden = false;
    if (!panel.hidden) scrollToEnd();
  };
}

// Keep a failure steady while commands retry or settings save in the background.
export function createStatusReporter({ render, recordError, schedule = setTimeout, cancel = clearTimeout }) {
  let timer = null;
  let failed = false;
  return (message, kind = "ready") => {
    if (failed && (kind === "busy" || message === "Saved")) return;
    cancel(timer);
    timer = null;
    if (kind === "error") {
      failed = true;
      render(summarizeError(message), kind);
      recordError(message);
    } else {
      timer = schedule(() => {
        timer = null;
        failed = false;
        render(message, kind);
      }, 200);
    }
  };
}
