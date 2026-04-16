function currentNavigatorPlatform(): string {
  return typeof navigator === "undefined" ? "" : navigator.platform;
}

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function isMacTauriWindow(): boolean {
  return isTauriRuntime() && /Mac|iPhone|iPad|iPod/.test(currentNavigatorPlatform());
}
