import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_THEME, THEMES, resolveTheme, themePreviewMarkup } from "../src/themes.js";

test("unknown or missing designs fall back to the default", () => {
  assert.equal(resolveTheme("aura"), "aura");
  assert.equal(resolveTheme("retired-design"), DEFAULT_THEME);
  assert.equal(resolveTheme(undefined), DEFAULT_THEME);
});

test("every design has a unique id and a preview tile", () => {
  assert.equal(new Set(THEMES.map((theme) => theme.id)).size, THEMES.length);
  for (const theme of THEMES) assert.match(themePreviewMarkup(theme.id), new RegExp(`data-theme-preview="${theme.id}"`));
});
