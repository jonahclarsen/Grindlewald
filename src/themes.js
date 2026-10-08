// Each design is one stylesheet in src/themes/ layered over styles.css, plus an always-loaded preview stylesheet.
export const THEMES = [
  { id: "haligonian", name: "Haligonian", description: "Teal bevels and pixel type" },
  { id: "lumen", name: "Lumen", description: "Native glass that follows macOS" },
  { id: "module", name: "Module", description: "A light hardware panel" },
  { id: "nocturne", name: "Nocturne", description: "Editorial serif type" },
  { id: "aura", name: "Aura", description: "Glows with your light" },
  { id: "phosphor", name: "Phosphor", description: "A terminal multiplexer" },
];

export const DEFAULT_THEME = THEMES[0].id;

export function resolveTheme(id) {
  return THEMES.some((theme) => theme.id === id) ? id : DEFAULT_THEME;
}

let requestedTheme = null;

// Load the new stylesheet before removing the old one so switching never flashes unstyled content.
export function applyTheme(id, doc = document) {
  const theme = resolveTheme(id);
  requestedTheme = theme;
  const href = `themes/${theme}.css`;
  const links = [...doc.querySelectorAll("link[data-theme-stylesheet]")];
  const current = links.find((link) => link.getAttribute("href") === href);
  const finish = (active) => {
    if (requestedTheme !== theme) return;
    doc.querySelectorAll("link[data-theme-stylesheet]").forEach((link) => { if (link !== active) link.remove(); });
    doc.documentElement.dataset.theme = theme;
  };
  if (current) {
    finish(current);
    return theme;
  }
  const next = doc.createElement("link");
  next.rel = "stylesheet";
  next.href = href;
  next.dataset.themeStylesheet = "";
  next.addEventListener("load", () => finish(next), { once: true });
  next.addEventListener("error", () => next.remove(), { once: true });
  (links.at(-1) ?? doc.head.lastElementChild).after(next);
  return theme;
}

export function themePreviewMarkup(id) {
  return `<span class="theme-preview" data-theme-preview="${id}" aria-hidden="true">
    <span class="tp-header"><span class="tp-mark"></span><span class="tp-title"></span></span>
    <span class="tp-body">
      <span class="tp-card tp-flood"><span class="tp-label"></span><span class="tp-button"></span><span class="tp-button tp-accent"></span></span>
      <span class="tp-card tp-color"><span class="tp-label"></span><span class="tp-swatch"></span><span class="tp-track tp-hue"><span class="tp-knob"></span></span></span>
      <span class="tp-card tp-white"><span class="tp-label"></span><span class="tp-swatch"></span><span class="tp-track tp-warm"><span class="tp-knob"></span></span></span>
      <span class="tp-card tp-power"><span class="tp-track tp-level"><span class="tp-knob"></span></span><span class="tp-button"></span><span class="tp-button tp-accent"></span></span>
    </span>
    <span class="tp-nav"><span class="tp-tab tp-active"></span><span class="tp-tab"></span><span class="tp-tab"></span></span>
  </span>`;
}
