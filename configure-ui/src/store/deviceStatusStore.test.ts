import test from "node:test";
import assert from "node:assert/strict";
import {
  deriveAppMode,
  deriveLocalPairingState,
  shouldPreservePairingAuthOnSessionChange,
  getAppMode,
  markPairingAuthValid,
  setDeviceProbeState,
  setDeviceSessionState,
  type AppMode,
  type AuthState,
  type DevicePairingState,
  type LocalPairingState,
  type TransportState,
} from "./deviceStatusStore.ts";

function resetStoreState() {
  setDeviceSessionState({ hasTarget: false, localPairing: "absent" });
  setDeviceProbeState({ transport: "none", devicePairing: "unknown" });
}

function expectAppMode(args: {
  baseUrl: string;
  transport: TransportState;
  devicePairing: DevicePairingState;
  localPairing: LocalPairingState;
  auth: AuthState;
  expected: AppMode;
}) {
  const { expected, ...input } = args;
  assert.equal(deriveAppMode(input), expected);
}

test("deriveLocalPairingState treats blank values as absent", () => {
  assert.equal(deriveLocalPairingState(""), "absent");
  assert.equal(deriveLocalPairingState("   "), "absent");
  assert.equal(deriveLocalPairingState(undefined), "absent");
});

test("deriveLocalPairingState treats six-digit local pairing as present", () => {
  assert.equal(deriveLocalPairingState("123456"), "present");
});

test("deriveAppMode returns no_target when device URL is missing", () => {
  expectAppMode({
    baseUrl: "",
    transport: "none",
    devicePairing: "unknown",
    localPairing: "absent",
    auth: "unknown",
    expected: "no_target",
  });
});

test("deriveAppMode returns probing while the device probe is in flight", () => {
  expectAppMode({
    baseUrl: "http://192.168.4.1",
    transport: "checking",
    devicePairing: "unknown",
    localPairing: "absent",
    auth: "unknown",
    expected: "probing",
  });
});

test("deriveAppMode returns offline when the configured device is unreachable", () => {
  expectAppMode({
    baseUrl: "http://192.168.4.1",
    transport: "unreachable",
    devicePairing: "unknown",
    localPairing: "present",
    auth: "unknown",
    expected: "offline",
  });
});

test("deriveAppMode returns init_pairing when the device pairing code is not initialized", () => {
  expectAppMode({
    baseUrl: "http://192.168.4.1",
    transport: "reachable",
    devicePairing: "uninitialized",
    localPairing: "absent",
    auth: "unknown",
    expected: "init_pairing",
  });
});

test("deriveAppMode returns unlock when the device is initialized but no local pairing code exists", () => {
  expectAppMode({
    baseUrl: "http://192.168.4.1",
    transport: "reachable",
    devicePairing: "initialized",
    localPairing: "absent",
    auth: "unknown",
    expected: "unlock",
  });
});

test("deriveAppMode returns unlock when local pairing exists but auth is still unknown", () => {
  expectAppMode({
    baseUrl: "http://192.168.4.1",
    transport: "reachable",
    devicePairing: "initialized",
    localPairing: "present",
    auth: "unknown",
    expected: "unlock",
  });
});

test("deriveAppMode returns unlock when pairing auth has been invalidated", () => {
  expectAppMode({
    baseUrl: "http://192.168.4.1",
    transport: "reachable",
    devicePairing: "initialized",
    localPairing: "present",
    auth: "invalid",
    expected: "unlock",
  });
});

test("deriveAppMode returns ready only when the device is reachable and protected auth is valid", () => {
  expectAppMode({
    baseUrl: "http://192.168.4.1",
    transport: "reachable",
    devicePairing: "initialized",
    localPairing: "present",
    auth: "valid",
    expected: "ready",
  });
});

test("first successful unlock reaches ready once the validated pairing is persisted on the same target", () => {
  resetStoreState();
  setDeviceSessionState({ hasTarget: true, localPairing: "absent" });
  setDeviceProbeState({ transport: "reachable", devicePairing: "initialized" });

  markPairingAuthValid();
  assert.equal(getAppMode(), "unlock");

  setDeviceSessionState({
    hasTarget: true,
    localPairing: "present",
    preserveAuth: true,
  });
  assert.equal(getAppMode(), "ready");

  resetStoreState();
});

test("protected validation promotes unknown pairing state before local code persists", () => {
  resetStoreState();
  setDeviceSessionState({ hasTarget: true, localPairing: "absent" });
  setDeviceProbeState({ transport: "reachable", devicePairing: "unknown" });

  markPairingAuthValid();
  assert.equal(getAppMode(), "unlock");

  setDeviceSessionState({
    hasTarget: true,
    localPairing: "present",
    preserveAuth: true,
  });
  assert.equal(getAppMode(), "ready");

  resetStoreState();
});

test("switching target clears previously validated pairing auth", () => {
  resetStoreState();
  setDeviceSessionState({ hasTarget: true, localPairing: "absent" });
  setDeviceProbeState({ transport: "reachable", devicePairing: "initialized" });
  markPairingAuthValid();
  setDeviceSessionState({
    hasTarget: true,
    localPairing: "present",
    preserveAuth: true,
  });
  assert.equal(getAppMode(), "ready");

  setDeviceSessionState({
    hasTarget: true,
    localPairing: "present",
    preserveAuth: false,
  });
  assert.equal(getAppMode(), "unlock");

  resetStoreState();
});

test("pairing auth preservation is scoped to the exact device session", () => {
  assert.equal(
    shouldPreservePairingAuthOnSessionChange({
      previousBaseUrl: "http://192.168.4.1",
      previousPairingCode: "123456",
      nextBaseUrl: "http://192.168.4.1/",
      nextPairingCode: "654321",
    }),
    false,
  );
  assert.equal(
    shouldPreservePairingAuthOnSessionChange({
      previousBaseUrl: "http://192.168.4.1",
      previousPairingCode: "123456",
      nextBaseUrl: "http://192.168.4.20",
      nextPairingCode: "123456",
    }),
    false,
  );
  assert.equal(
    shouldPreservePairingAuthOnSessionChange({
      previousBaseUrl: "http://192.168.4.1",
      previousPairingCode: "123456",
      nextBaseUrl: "http://192.168.4.1",
      nextPairingCode: "123456",
    }),
    true,
  );
});

test("pairing auth preservation allows a validated empty-to-present unlock promotion", () => {
  assert.equal(
    shouldPreservePairingAuthOnSessionChange({
      previousBaseUrl: "http://192.168.4.1",
      previousPairingCode: "",
      nextBaseUrl: "http://192.168.4.1",
      nextPairingCode: "123456",
    }),
    true,
  );
});
