import test from "node:test";
import assert from "node:assert/strict";
import {
  deriveDeviceSetupCardModel,
  deriveDeviceAccessStage,
  deriveSuccessfulProbeSessionUpdate,
  normalizeDeviceUrl,
  validatePairingCodeDraft,
} from "./deviceAccessFlow.ts";

test("deriveDeviceAccessStage keeps connect/probe/offline in the connection stage", () => {
  assert.equal(deriveDeviceAccessStage("no_target"), "connect");
  assert.equal(deriveDeviceAccessStage("probing"), "connect");
  assert.equal(deriveDeviceAccessStage("offline"), "connect");
});

test("deriveDeviceAccessStage maps init_pairing and unlock to dedicated onboarding stages", () => {
  assert.equal(deriveDeviceAccessStage("init_pairing"), "init_pairing");
  assert.equal(deriveDeviceAccessStage("unlock"), "unlock");
  assert.equal(deriveDeviceAccessStage("ready"), "ready");
});

test("deriveDeviceSetupCardModel keeps the initial card focused on connection detection", () => {
  const connectModel = deriveDeviceSetupCardModel("no_target");
  assert.deepEqual(connectModel, {
    stage: "connect",
    primaryAction: "probe",
    showPairingCodeField: false,
    requireExplicitConfirmation: false,
  });

  const offlineModel = deriveDeviceSetupCardModel("offline");
  assert.deepEqual(offlineModel, {
    stage: "connect",
    primaryAction: "probe",
    showPairingCodeField: false,
    requireExplicitConfirmation: false,
  });
});

test("deriveDeviceSetupCardModel reveals the pairing input only after detection succeeds", () => {
  assert.deepEqual(deriveDeviceSetupCardModel("unlock"), {
    stage: "unlock",
    primaryAction: "save_existing_pairing",
    showPairingCodeField: true,
    requireExplicitConfirmation: false,
  });

  assert.deepEqual(deriveDeviceSetupCardModel("init_pairing"), {
    stage: "init_pairing",
    primaryAction: "save_new_pairing",
    showPairingCodeField: true,
    requireExplicitConfirmation: true,
  });
});

test("deriveDeviceSetupCardModel falls back to connection detection when the device URL draft changes", () => {
  assert.deepEqual(
    deriveDeviceSetupCardModel("unlock", { targetDirty: true }),
    {
      stage: "connect",
      primaryAction: "probe",
      showPairingCodeField: false,
      requireExplicitConfirmation: false,
    },
  );
});

test("normalizeDeviceUrl trims whitespace and falls back to the default ESP URL", () => {
  assert.equal(normalizeDeviceUrl("  http://192.168.1.20/ "), "http://192.168.1.20");
  assert.equal(normalizeDeviceUrl(""), "http://192.168.4.1");
});

test("validatePairingCodeDraft requires exactly six digits", () => {
  assert.equal(validatePairingCodeDraft(""), "device.pairingCodeRequired");
  assert.equal(validatePairingCodeDraft("12345"), "device.pairingCodeInvalid");
  assert.equal(validatePairingCodeDraft("12a456"), "device.pairingCodeInvalid");
  assert.equal(validatePairingCodeDraft("123456"), null);
});

test("deriveSuccessfulProbeSessionUpdate preserves local pairing only for the same initialized target", () => {
  assert.deepEqual(
    deriveSuccessfulProbeSessionUpdate({
      probedUrl: "http://192.168.4.1",
      currentBaseUrl: "http://192.168.4.1",
      currentPairingCode: "123456",
      codeSet: true,
    }),
    {
      nextBaseUrl: "http://192.168.4.1",
      detectedPairingState: "initialized",
      nextLocalPairing: "present",
      preserveAuth: true,
      shouldClearStoredPairing: false,
    },
  );
});

test("deriveSuccessfulProbeSessionUpdate clears stale pairing when the target changes", () => {
  assert.deepEqual(
    deriveSuccessfulProbeSessionUpdate({
      probedUrl: "http://192.168.4.20",
      currentBaseUrl: "http://192.168.4.1",
      currentPairingCode: "123456",
      codeSet: true,
    }),
    {
      nextBaseUrl: "http://192.168.4.20",
      detectedPairingState: "initialized",
      nextLocalPairing: "absent",
      preserveAuth: false,
      shouldClearStoredPairing: true,
    },
  );
});

test("deriveSuccessfulProbeSessionUpdate clears stale pairing when the device is no longer initialized", () => {
  assert.deepEqual(
    deriveSuccessfulProbeSessionUpdate({
      probedUrl: "http://192.168.4.1",
      currentBaseUrl: "http://192.168.4.1",
      currentPairingCode: "123456",
      codeSet: false,
    }),
    {
      nextBaseUrl: "http://192.168.4.1",
      detectedPairingState: "uninitialized",
      nextLocalPairing: "absent",
      preserveAuth: false,
      shouldClearStoredPairing: true,
    },
  );
});
