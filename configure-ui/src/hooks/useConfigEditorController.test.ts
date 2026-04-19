import assert from "node:assert/strict";
import test from "node:test";
import { runValidatedConfigSave } from "./configSaveLifecycle.ts";

test("runValidatedConfigSave short-circuits when validation fails", async () => {
  const calls: string[] = [];
  const result = await runValidatedConfigSave({
    validate: () => "validation failed",
    saveFeedback: {
      begin: () => calls.push("begin"),
      fail: (message) => calls.push(`fail:${message}`),
      finishFromResult: () => calls.push("finish"),
    },
    performSave: async () => {
      calls.push("save");
      return { ok: true };
    },
    markClean: () => calls.push("clean"),
    onBeforeSave: () => calls.push("before"),
    onSuccess: () => calls.push("success"),
  });

  assert.equal(result, null);
  assert.deepEqual(calls, ["fail:validation failed"]);
});

test("runValidatedConfigSave drives save lifecycle and marks the form clean on success", async () => {
  const calls: string[] = [];
  const result = await runValidatedConfigSave({
    validate: () => null,
    saveFeedback: {
      begin: () => calls.push("begin"),
      fail: (message) => calls.push(`fail:${message}`),
      finishFromResult: (saveResult) => calls.push(`finish:${saveResult.ok}`),
    },
    performSave: async () => {
      calls.push("save");
      return { ok: true, restartRequired: true };
    },
    markClean: () => calls.push("clean"),
    onBeforeSave: () => calls.push("before"),
    onSuccess: (saveResult) => calls.push(`success:${saveResult.restartRequired}`),
  });

  assert.deepEqual(result, { ok: true, restartRequired: true });
  assert.deepEqual(calls, [
    "begin",
    "before",
    "save",
    "finish:true",
    "clean",
    "success:true",
  ]);
});
