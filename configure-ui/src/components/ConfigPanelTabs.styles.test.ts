import test from "node:test";
import assert from "node:assert/strict";
import {
  CONFIG_PANEL_TAB_INDICATOR_SX,
  createConfigPanelTabsSx,
} from "./ConfigPanelTabs.styles.ts";
import { LAYOUT_TOKENS } from "../config/themeTokens.ts";

test("config panel tabs use recessed track and hidden material indicator", () => {
  assert.deepEqual(CONFIG_PANEL_TAB_INDICATOR_SX, { display: "none" });

  const sx = createConfigPanelTabsSx({ minTabWidth: 88 });

  assert.equal(sx.width, "fit-content");
  assert.equal(sx.maxWidth, "100%");
  assert.equal(
    sx.minHeight,
    LAYOUT_TOKENS.configPanelTabHeightPx +
      LAYOUT_TOKENS.configPanelTabsInsetPx * 2,
  );
  assert.equal(sx["& .MuiTab-root"].minWidth, 88);
  assert.equal(sx["& .MuiTab-root"].letterSpacing, 0);
  assert.equal(sx["& .MuiTab-root.Mui-selected"].boxShadow, "var(--os3d-control-soft-lift-stack)");
});
