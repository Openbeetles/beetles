/**
 * 设备状态单一数据源 / Single source of truth for device access state.
 *
 * 这层现在显式区分：
 * - 传输连通性（transport）
 * - 设备端配对码初始化状态（devicePairing）
 * - 本地是否缓存配对码（localPairing）
 * - 本地配对码是否已被受保护 API 验证（auth）
 *
 * 这些源状态再统一派生出全局 `appMode`，由 shell 级 UI 消费。
 */

import { useSyncExternalStore } from "react";

export type TransportState = "none" | "checking" | "reachable" | "unreachable";
/** Back-compat alias for existing consumers that still read `connectionStatus`. */
export type ConnectionStatus = TransportState;
export type DevicePairingState = "unknown" | "uninitialized" | "initialized";
export type LocalPairingState = "absent" | "present";
export type AuthState = "unknown" | "valid" | "invalid";
export type AppMode =
  | "no_target"
  | "probing"
  | "offline"
  | "init_pairing"
  | "unlock"
  | "ready";

/**
 * 设备运行平台（由固件 `GET /api/system_info` 的 `board_id` 推断），供 UI 按平台裁剪表单项。
 * Inferred from firmware `board_id`: `linux` → linux, `esp32-*` / `unsupported-soc-*` → esp, `host` → 本机调试二进制。
 */
export type DeviceRuntimeKind = "unknown" | "linux" | "esp" | "host";

interface DeviceStatusSource {
  hasTarget: boolean;
  transport: TransportState;
  devicePairing: DevicePairingState;
  localPairing: LocalPairingState;
  auth: AuthState;
  runtimeKind: DeviceRuntimeKind;
}

export interface DeviceStatus extends DeviceStatusSource {
  /** Legacy alias kept during migration. */
  connectionStatus: ConnectionStatus;
  /** Legacy alias kept during migration. */
  activated: boolean | null;
  deviceConnected: boolean;
  appMode: AppMode;
}

/** 重启闭环阶段：idle=无重启流程，pending=指令已发设备仍可达，restarting=设备已掉线等待上线 */
export type RestartPhase = "idle" | "pending" | "restarting";

const RESTART_TIMEOUT_MS = 60_000;
/**
 * 兜底：若指令发出后一直处于 reachable（可能因轮询采样未命中掉线窗口），
 * 超过该时长自动结束 pending，避免蒙层卡死。
 */
const RESTART_PENDING_MAX_MS = 90_000;
const RESTART_REACHABLE_CONFIRM_MS = 15_000;

function deriveActivated(devicePairing: DevicePairingState): boolean | null {
  if (devicePairing === "initialized") return true;
  if (devicePairing === "uninitialized") return false;
  return null;
}

function hasTargetFromBaseUrl(baseUrl: string | undefined): boolean {
  return Boolean(baseUrl?.trim());
}

function normalizeDeviceSessionBaseUrl(baseUrl: string | undefined): string {
  return (baseUrl ?? "").trim().replace(/\/$/, "");
}

function normalizeDeviceSessionPairingCode(
  pairingCode: string | null | undefined,
): string {
  return pairingCode?.trim() ?? "";
}

export function deriveLocalPairingState(
  pairingCode: string | null | undefined,
): LocalPairingState {
  return pairingCode?.trim() ? "present" : "absent";
}

export function shouldPreservePairingAuthOnSessionChange(args: {
  previousBaseUrl: string | undefined;
  previousPairingCode: string | null | undefined;
  nextBaseUrl: string | undefined;
  nextPairingCode: string | null | undefined;
}): boolean {
  const previousBaseUrl = normalizeDeviceSessionBaseUrl(args.previousBaseUrl);
  const nextBaseUrl = normalizeDeviceSessionBaseUrl(args.nextBaseUrl);
  if (!previousBaseUrl || previousBaseUrl !== nextBaseUrl) return false;

  const previousCode = normalizeDeviceSessionPairingCode(
    args.previousPairingCode,
  );
  const nextCode = normalizeDeviceSessionPairingCode(args.nextPairingCode);
  if (previousCode === nextCode) return true;

  // Unlock/init validates a transient code before persisting it, so the only
  // allowed code-changing preservation is the explicit absent -> present lift.
  return previousCode === "" && nextCode !== "";
}

export function deriveAppMode(args: {
  baseUrl: string;
  transport: TransportState;
  devicePairing: DevicePairingState;
  localPairing: LocalPairingState;
  auth: AuthState;
}): AppMode {
  const { baseUrl, transport, devicePairing, localPairing, auth } = args;
  if (!hasTargetFromBaseUrl(baseUrl)) return "no_target";
  if (transport === "checking" || transport === "none") return "probing";
  if (transport === "unreachable") return "offline";
  if (devicePairing === "uninitialized") return "init_pairing";
  if (devicePairing !== "initialized") return "probing";
  if (localPairing === "absent") return "unlock";
  if (auth !== "valid") return "unlock";
  return "ready";
}

function deriveAppModeFromSource(source: DeviceStatusSource): AppMode {
  return deriveAppMode({
    baseUrl: source.hasTarget ? "configured-target" : "",
    transport: source.transport,
    devicePairing: source.devicePairing,
    localPairing: source.localPairing,
    auth: source.auth,
  });
}

function buildStatusSnapshot(source: DeviceStatusSource): DeviceStatus {
  const activated = deriveActivated(source.devicePairing);
  return {
    ...source,
    connectionStatus: source.transport,
    activated,
    deviceConnected: source.transport === "reachable",
    appMode: deriveAppModeFromSource(source),
  };
}

let source: DeviceStatusSource = {
  hasTarget: false,
  transport: "none",
  devicePairing: "unknown",
  localPairing: "absent",
  auth: "unknown",
  runtimeKind: "unknown",
};

let status = buildStatusSnapshot(source);
let restartPending = false;
let restartPendingSince: number | null = null;
let restartDropTime: number | null = null;
let reconnectedAfterRestart = false;
let restartTimeout = false;
const listeners = new Set<() => void>();
const restartListeners = new Set<() => void>();

/** 由 `board_id` 推断平台；与固件 `runtime_board::resolved_board_id()` 约定一致。 */
export function inferDeviceRuntimeKind(
  boardId: string | undefined,
): DeviceRuntimeKind {
  const id = boardId?.trim();
  if (!id) return "unknown";
  if (id === "linux") return "linux";
  if (id === "host") return "host";
  if (id.startsWith("esp32-") || id.startsWith("unsupported-soc-")) return "esp";
  return "unknown";
}

function getSnapshot(): DeviceStatus {
  return status;
}

function getRuntimeKindSnapshot(): DeviceRuntimeKind {
  return status.runtimeKind;
}

function getAppModeSnapshot(): AppMode {
  return status.appMode;
}

function subscribe(callback: () => void): () => void {
  listeners.add(callback);
  return () => listeners.delete(callback);
}

function subscribeRestart(cb: () => void): () => void {
  restartListeners.add(cb);
  return () => restartListeners.delete(cb);
}

function emitChange(): void {
  listeners.forEach((listener) => listener());
  restartListeners.forEach((listener) => listener());
}

function commitSource(next: DeviceStatusSource): void {
  const nextStatus = buildStatusSnapshot(next);
  const prev = status;
  source = next;
  status = nextStatus;
  if (
    prev.connectionStatus === nextStatus.connectionStatus &&
    prev.activated === nextStatus.activated &&
    prev.runtimeKind === nextStatus.runtimeKind &&
    prev.hasTarget === nextStatus.hasTarget &&
    prev.localPairing === nextStatus.localPairing &&
    prev.auth === nextStatus.auth &&
    prev.appMode === nextStatus.appMode
  ) {
    return;
  }
  emitChange();
}

function clearRestartLifecycle(): void {
  restartPending = false;
  restartPendingSince = null;
  restartDropTime = null;
}

export function setDeviceSessionState(args: {
  hasTarget: boolean;
  localPairing: LocalPairingState;
  preserveAuth?: boolean;
}): void {
  const { hasTarget, localPairing, preserveAuth = false } = args;
  if (!hasTarget) {
    clearRestartLifecycle();
    commitSource({
      ...source,
      hasTarget: false,
      transport: "none",
      devicePairing: "unknown",
      localPairing,
      auth: "unknown",
      runtimeKind: "unknown",
    });
    return;
  }
  commitSource({
    ...source,
    hasTarget: true,
    localPairing,
    auth:
      localPairing === "present" && preserveAuth ? source.auth : "unknown",
  });
}

export function setLocalPairingState(localPairing: LocalPairingState): void {
  commitSource({
    ...source,
    localPairing,
    auth: localPairing === "present" ? "unknown" : "unknown",
  });
}

export function resetPairingAuthState(): void {
  if (source.auth === "unknown") return;
  commitSource({ ...source, auth: "unknown" });
}

export function markPairingAuthValid(): void {
  if (!source.hasTarget) return;
  if (
    source.transport === "reachable" &&
    source.devicePairing === "initialized" &&
    source.auth === "valid"
  ) {
    return;
  }
  // A protected API success proves both transport reachability and initialized pairing.
  commitSource({
    ...source,
    transport: "reachable",
    devicePairing: "initialized",
    auth: "valid",
  });
}

export function markPairingAuthInvalid(): void {
  if (source.localPairing !== "present") return;
  if (source.auth === "invalid") return;
  commitSource({ ...source, auth: "invalid" });
}

export function setDeviceProbeState(args: {
  transport: TransportState;
  devicePairing: DevicePairingState;
}): void {
  const { transport, devicePairing } = args;
  commitSource({
    ...source,
    transport,
    devicePairing,
    auth: devicePairing === "initialized" ? source.auth : "unknown",
  });
}

/**
 * 旧接口兼容层：仍允许 `DeviceProvider` 先按旧签名写入，
 * 后续调用链再逐步迁到显式的 `setDeviceSessionState` + `setDeviceProbeState`。
 */
export function setDeviceStatus(
  connectionStatus: ConnectionStatus,
  activated: boolean | null,
): void {
  const devicePairing =
    activated === true
      ? "initialized"
      : activated === false
        ? "uninitialized"
        : "unknown";
  setDeviceProbeState({ transport: connectionStatus, devicePairing });
}

/** 在拿到 system_info 后调用（如 DevicePage）。 */
export function setDeviceRuntimeKindFromBoardId(
  boardId: string | undefined,
): void {
  const next = inferDeviceRuntimeKind(boardId);
  if (source.runtimeKind === next) return;
  commitSource({ ...source, runtimeKind: next });
}

/** baseUrl 切换或清空时由 DeviceProvider 调用，避免沿用上一条连接的平台。 */
export function resetDeviceRuntimeKind(): void {
  if (source.runtimeKind === "unknown") return;
  commitSource({ ...source, runtimeKind: "unknown" });
}

/** 配置刷新成功后可调用：立即把连接态标记为可达，避免 UI 继续显示断连蒙层。 */
export function markDeviceReachable(): void {
  if (source.transport === "reachable") return;
  commitSource({ ...source, transport: "reachable" });
}

/** 重启指令已发出，由 TopBar 在 POST /api/restart 成功后调用。 */
export function setRestartPending(): void {
  restartPending = true;
  restartPendingSince = Date.now();
  restartDropTime = null;
  emitChange();
}

/**
 * 轮询得到新连接状态后调用，用于推进重启闭环：不可达时记 dropTime，再次可达则置「重启完成」并清 pending；
 * 自 drop 起超过 1 分钟仍不可达则置「设备可能异常」并清 pending。
 */
export function updateRestartState(connectionStatus: ConnectionStatus): void {
  if (!restartPending) return;
  const now = Date.now();
  if (
    connectionStatus === "reachable" &&
    (restartDropTime !== null ||
      (restartPendingSince !== null &&
        now - restartPendingSince >= RESTART_REACHABLE_CONFIRM_MS))
  ) {
    clearRestartLifecycle();
    reconnectedAfterRestart = true;
    emitChange();
    return;
  }
  if (
    restartDropTime === null &&
    restartPendingSince !== null &&
    now - restartPendingSince > RESTART_PENDING_MAX_MS
  ) {
    clearRestartLifecycle();
    restartTimeout = true;
    emitChange();
    return;
  }
  if (connectionStatus === "unreachable") {
    if (restartDropTime === null) restartDropTime = now;
    else if (now - restartDropTime > RESTART_TIMEOUT_MS) {
      clearRestartLifecycle();
      restartTimeout = true;
      emitChange();
    }
  }
}

/** 当前重启阶段，供横幅等展示。 */
export function getRestartPhase(): RestartPhase {
  if (!restartPending) return "idle";
  return restartDropTime === null ? "pending" : "restarting";
}

/** 一次性：重启后已重新连接，消费后清除。用于弹 toast「重启完成」。 */
export function consumeReconnectedAfterRestart(): boolean {
  const value = reconnectedAfterRestart;
  if (value) {
    reconnectedAfterRestart = false;
    emitChange();
  }
  return value;
}

/** 一次性：重启流程超时，消费后清除。用于弹 toast「设备可能异常」。 */
export function consumeRestartTimeout(): boolean {
  const value = restartTimeout;
  if (value) {
    restartTimeout = false;
    emitChange();
  }
  return value;
}

/** 订阅重启阶段变化（phase）；单次事件由上层用 consume* 消费。 */
export function useRestartPhase(): RestartPhase {
  return useSyncExternalStore(
    subscribeRestart,
    getRestartPhase,
    getRestartPhase,
  );
}

/** 在组件中订阅完整设备状态。 */
export function useDeviceStatus(): DeviceStatus {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

/** 仅订阅运行平台（Linux / ESP / host / unknown），用于表单项显隐。 */
export function useDeviceRuntimeKind(): DeviceRuntimeKind {
  return useSyncExternalStore(
    subscribe,
    getRuntimeKindSnapshot,
    getRuntimeKindSnapshot,
  );
}

/** 全局应用模式：由 shell 级 UI 消费。 */
export function useAppMode(): AppMode {
  return useSyncExternalStore(subscribe, getAppModeSnapshot, getAppModeSnapshot);
}

/** 设备是否已连接（可达且已拿到 pairing_code 响应）。 */
export function useDeviceConnected(): boolean {
  return useDeviceStatus().deviceConnected;
}

/** 非 React 环境获取当前是否已连接。 */
export function getDeviceConnected(): boolean {
  return status.deviceConnected;
}

/** 非 React 环境获取当前应用模式。 */
export function getAppMode(): AppMode {
  return status.appMode;
}
