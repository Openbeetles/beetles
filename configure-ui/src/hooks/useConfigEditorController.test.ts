import assert from "node:assert/strict";
import test from "node:test";
import {
  CONFIG_SAVE_IN_PROGRESS_ERROR,
  runSingleFlightConfigSave,
  runValidatedConfigSave,
} from "./configSaveLifecycle.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

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

test("runSingleFlightConfigSave rejects duplicate triggers while the first save is in flight", async () => {
  const save = deferred<{ ok: boolean }>();
  const slot = { current: null as Promise<{ ok: boolean }> | null };
  let calls = 0;

  const first = runSingleFlightConfigSave(slot, async () => {
    calls += 1;
    return save.promise;
  });
  const second = runSingleFlightConfigSave(slot, async () => {
    calls += 1;
    return { ok: true };
  });

  assert.equal(calls, 1);
  assert.deepEqual(await second, {
    ok: false,
    error: CONFIG_SAVE_IN_PROGRESS_ERROR,
  });

  save.resolve({ ok: true });
  assert.deepEqual(await first, { ok: true });
  assert.equal(slot.current, null);
});

test("runSingleFlightConfigSave allows retry after a failed save settles", async () => {
  const slot = { current: null as Promise<{ ok: boolean; error?: string }> | null };
  let calls = 0;

  const failed = await runSingleFlightConfigSave(slot, async () => {
    calls += 1;
    return { ok: false, error: "failed" };
  });
  const retried = await runSingleFlightConfigSave(slot, async () => {
    calls += 1;
    return { ok: true };
  });

  assert.deepEqual(failed, { ok: false, error: "failed" });
  assert.deepEqual(retried, { ok: true });
  assert.equal(calls, 2);
});

test("runSingleFlightConfigSave allows callers to clear the slot for a new device session", async () => {
  const firstSave = deferred<{ ok: boolean }>();
  const slot = { current: null as Promise<{ ok: boolean }> | null };
  let calls = 0;

  const first = runSingleFlightConfigSave(slot, async () => {
    calls += 1;
    return firstSave.promise;
  });
  slot.current = null;
  const second = runSingleFlightConfigSave(slot, async () => {
    calls += 1;
    return { ok: true };
  });

  assert.notEqual(first, second);
  assert.equal(calls, 2);
  assert.deepEqual(await second, { ok: true });
  firstSave.resolve({ ok: true });
  assert.deepEqual(await first, { ok: true });
});
