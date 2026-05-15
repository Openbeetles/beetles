import type {
  AppMode,
  RestartPhase,
} from "../store/deviceStatusStore";

export function shouldShowProtectedRouteBlocker(args: {
  pathname: string;
  appMode: AppMode;
  deviceConnected: boolean;
  restartPhase: RestartPhase;
}): boolean {
  if (args.pathname === "/device") return false;
  if (args.restartPhase !== "idle") return false;
  if (!args.deviceConnected) return true;
  if (args.appMode === "ready") return false;
  return true;
}
