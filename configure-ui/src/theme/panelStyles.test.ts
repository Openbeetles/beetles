import test from "node:test";
import assert from "node:assert/strict";

import { FORM_SWITCH_ROW_SX, TEXT_OVERLINE_SX } from "./panelStyles.ts";

test("form switch rows stay compact with the switch before the label", () => {
  assert.equal(FORM_SWITCH_ROW_SX.width, "fit-content");
  assert.equal(FORM_SWITCH_ROW_SX.justifyContent, "flex-start");
  assert.equal(
    FORM_SWITCH_ROW_SX["& .MuiFormControlLabel-label"].flex,
    "0 1 auto",
  );
});

test("overline text token centralizes uppercase dashboard labels", () => {
  assert.equal(TEXT_OVERLINE_SX.fontSize, "var(--font-size-caption)");
  assert.equal(TEXT_OVERLINE_SX.letterSpacing, "var(--letter-spacing-small)");
  assert.equal(TEXT_OVERLINE_SX.textTransform, "uppercase");
});
