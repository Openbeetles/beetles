import assert from "node:assert/strict";
import test from "node:test";
import { shouldCloseChatDialog } from "./ChatDialogModel.ts";

test("shouldCloseChatDialog closes only for explicit chat window close requests", () => {
  assert.equal(shouldCloseChatDialog("explicit"), true);
  assert.equal(shouldCloseChatDialog("backdropClick"), false);
  assert.equal(shouldCloseChatDialog("escapeKeyDown"), false);
});
