import type {
  AppMode,
  DevicePairingState,
  LocalPairingState,
} from "../store/deviceStatusStore";

export const DEFAULT_DEVICE_BASE_URL = "http://192.168.4.1";

export type DeviceAccessStage = "connect" | "init_pairing" | "unlock" | "ready";
export type DeviceSetupPrimaryAction =
  | "probe"
  | "save_existing_pairing"
  | "save_new_pairing";

export interface DeviceSetupCardModel {
  stage: Exclude<DeviceAccessStage, "ready">;
  primaryAction: DeviceSetupPrimaryAction;
  showPairingCodeField: boolean;
  requireExplicitConfirmation: boolean;
}

export interface SuccessfulProbeSessionUpdate {
  nextBaseUrl: string;
  detectedPairingState: DevicePairingState;
  nextLocalPairing: LocalPairingState;
  preserveAuth: boolean;
  shouldClearStoredPairing: boolean;
}

function normalizePairingCode(code: string): string {
  return code.trim();
}

export function normalizeDeviceUrl(value: string): string {
  return value.trim().replace(/\/$/, "") || DEFAULT_DEVICE_BASE_URL;
}

export function deriveDeviceAccessStage(appMode: AppMode): DeviceAccessStage {
  if (appMode === "init_pairing") return "init_pairing";
  if (appMode === "unlock") return "unlock";
  if (appMode === "ready") return "ready";
  return "connect";
}

export function deriveDeviceSetupCardModel(
  appMode: AppMode,
  options: { targetDirty?: boolean } = {},
): DeviceSetupCardModel {
  if (options.targetDirty) {
    return {
      stage: "connect",
      primaryAction: "probe",
      showPairingCodeField: false,
      requireExplicitConfirmation: false,
    };
  }
  const stage = deriveDeviceAccessStage(appMode);
  if (stage === "init_pairing") {
    return {
      stage,
      primaryAction: "save_new_pairing",
      showPairingCodeField: true,
      requireExplicitConfirmation: true,
    };
  }
  if (stage === "unlock") {
    return {
      stage,
      primaryAction: "save_existing_pairing",
      showPairingCodeField: true,
      requireExplicitConfirmation: false,
    };
  }
  return {
    stage: "connect",
    primaryAction: "probe",
    showPairingCodeField: false,
    requireExplicitConfirmation: false,
  };
}

export function validatePairingCodeDraft(code: string): string | null {
  const normalized = normalizePairingCode(code);
  if (!normalized) return "device.pairingCodeRequired";
  if (!/^\d{6}$/.test(normalized)) return "device.pairingCodeInvalid";
  return null;
}

export function deriveSuccessfulProbeSessionUpdate(args: {
  probedUrl: string;
  currentBaseUrl?: string;
  currentPairingCode?: string;
  codeSet: boolean;
}): SuccessfulProbeSessionUpdate {
  const nextBaseUrl = normalizeDeviceUrl(args.probedUrl);
  const currentBaseUrl = normalizeDeviceUrl(args.currentBaseUrl ?? "");
  const hasStoredPairing = normalizePairingCode(args.currentPairingCode ?? "") !== "";
  const sameTarget = currentBaseUrl === nextBaseUrl && currentBaseUrl !== "";
  const preserveStoredPairing = args.codeSet && sameTarget && hasStoredPairing;

  return {
    nextBaseUrl,
    detectedPairingState: args.codeSet ? "initialized" : "uninitialized",
    nextLocalPairing: preserveStoredPairing ? "present" : "absent",
    preserveAuth: preserveStoredPairing,
    shouldClearStoredPairing: hasStoredPairing && !preserveStoredPairing,
  };
}
