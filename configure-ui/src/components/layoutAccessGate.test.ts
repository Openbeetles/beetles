import test from "node:test";
import assert from "node:assert/strict";
import { shouldShowProtectedRouteBlocker } from "./layoutAccessGate.ts";

test("shows the shared access card for disconnected protected routes", () => {
  assert.equal(
    shouldShowProtectedRouteBlocker({
      pathname: "/ai-config",
      appMode: "ready",
      deviceConnected: false,
      restartPhase: "idle",
    }),
    true,
  );
});

test("does not block the device page itself", () => {
  assert.equal(
    shouldShowProtectedRouteBlocker({
      pathname: "/device",
      appMode: "offline",
      deviceConnected: false,
      restartPhase: "idle",
    }),
    false,
  );
});

test("does not replace the shell while restart flow is active", () => {
  assert.equal(
    shouldShowProtectedRouteBlocker({
      pathname: "/system",
      appMode: "offline",
      deviceConnected: false,
      restartPhase: "restarting",
    }),
    false,
  );
});
