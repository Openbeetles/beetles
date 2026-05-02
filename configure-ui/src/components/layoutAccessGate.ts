import type {
  AppMode,
  AuthState,
  LocalPairingState,
  RestartPhase,
} from "../store/deviceStatusStore";

export function shouldShowProtectedRouteBlocker(args: {
  pathname: string;
  appMode: AppMode;
  deviceConnected: boolean;
  restartPhase: RestartPhase;
  localPairing?: LocalPairingState;
  auth?: AuthState;
}): boolean {
  if (args.pathname === "/device") return false;
  if (args.restartPhase !== "idle") return false;
  if (!args.deviceConnected) return true;
  if (args.appMode === "ready") return false;
  if (
    args.appMode === "unlock" &&
    args.localPairing === "present" &&
    args.auth === "unknown"
  ) {
    return false;
  }
  return true;
}
