import test from "node:test";
import assert from "node:assert/strict";
import { yesNo } from "./deviceStatusBarHelpers.ts";

const t = (key: string) => {
  switch (key) {
    case "common.yes":
      return "Yes";
    case "common.no":
      return "No";
    case "common.na":
      return "N/A";
    default:
      return key;
  }
};

test("yesNo remains available for shared device status rendering", () => {
  assert.equal(yesNo(true, t), "Yes");
  assert.equal(yesNo(false, t), "No");
  assert.equal(yesNo(undefined, t), "N/A");
});
