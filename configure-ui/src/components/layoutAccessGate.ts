import type { AppMode, RestartPhase } from "../store/deviceStatusStore";

export function shouldShowProtectedRouteBlocker(args: {
  pathname: string;
  appMode: AppMode;
  deviceConnected: boolean;
  restartPhase: RestartPhase;
}): boolean {
  return (
    args.pathname !== "/device" &&
    (args.appMode !== "ready" || !args.deviceConnected) &&
    args.restartPhase === "idle"
  );
}
