import assert from "node:assert/strict";
import test from "node:test";
import { shouldCloseChatDialog } from "./ChatDialogModel.ts";

test("shouldCloseChatDialog allows backdrop and escape closing for the chat window", () => {
  assert.equal(shouldCloseChatDialog("explicit"), true);
  assert.equal(shouldCloseChatDialog("backdropClick"), true);
  assert.equal(shouldCloseChatDialog("escapeKeyDown"), true);
});
