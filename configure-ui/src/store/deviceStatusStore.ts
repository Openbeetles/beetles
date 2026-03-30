/**
 * 设备状态单一数据源：连接状态 + 是否激活（设备端是否已设配对码）+ 重启闭环状态。
 * 仅由 DeviceProvider 写入：baseUrl 变化时检测一次，并有定时复检（10s，见 DeviceProvider），
 * 便于设备断线后更新侧栏/横幅；各页通过 useDeviceStatus() / useDeviceConnected() 消费。
 * 重启闭环：指令发出→仍可达=正在重启→不可达=重启中→再次可达=重启完成；超时 1 分钟=设备可能异常。
 */

import { useSyncExternalStore } from 'react'

export type ConnectionStatus = 'none' | 'checking' | 'reachable' | 'unreachable'

/**
 * 设备运行平台（由固件 `GET /api/system_info` 的 `board_id` 推断），供 UI 按平台裁剪表单项。
 * Inferred from firmware `board_id`: `linux` → linux, `esp32-*` / `unsupported-soc-*` → esp, `host` → 本机调试二进制。
 */
export type DeviceRuntimeKind = 'unknown' | 'linux' | 'esp' | 'host'

export interface DeviceStatus {
  connectionStatus: ConnectionStatus
  /** 设备端是否已设配对码（GET /api/pairing_code 的 code_set）；null = 未请求或不可达 */
  activated: boolean | null
  /** 在成功拉取 system_info 并解析 board_id 之前为 unknown */
  runtimeKind: DeviceRuntimeKind
}

/** 重启闭环阶段：idle=无重启流程，pending=指令已发设备仍可达，restarting=设备已掉线等待上线 */
export type RestartPhase = 'idle' | 'pending' | 'restarting'

const RESTART_TIMEOUT_MS = 60_000
/**
 * 兜底：若指令发出后一直处于 reachable（可能因轮询采样未命中掉线窗口），
 * 超过该时长自动结束 pending，避免蒙层卡死。
 */
const RESTART_PENDING_MAX_MS = 90_000

let status: DeviceStatus = {
  connectionStatus: 'none',
  activated: null,
  runtimeKind: 'unknown',
}

/** 由 `board_id` 推断平台；与固件 `runtime_board::resolved_board_id()` 约定一致。 */
export function inferDeviceRuntimeKind(boardId: string | undefined): DeviceRuntimeKind {
  const id = boardId?.trim()
  if (!id) return 'unknown'
  if (id === 'linux') return 'linux'
  if (id === 'host') return 'host'
  if (id.startsWith('esp32-') || id.startsWith('unsupported-soc-')) return 'esp'
  return 'unknown'
}

/** 在拿到 system_info 后调用（如 DevicePage）。 */
export function setDeviceRuntimeKindFromBoardId(boardId: string | undefined): void {
  const next = inferDeviceRuntimeKind(boardId)
  if (status.runtimeKind === next) return
  status = { ...status, runtimeKind: next }
  emitChange()
}

/** baseUrl 切换或清空时由 DeviceProvider 调用，避免沿用上一条连接的平台。 */
export function resetDeviceRuntimeKind(): void {
  if (status.runtimeKind === 'unknown') return
  status = { ...status, runtimeKind: 'unknown' }
  emitChange()
}

function getRuntimeKindSnapshot(): DeviceRuntimeKind {
  return status.runtimeKind
}
let restartPending = false
let restartPendingSince: number | null = null
let restartDropTime: number | null = null
let reconnectedAfterRestart = false
let restartTimeout = false
const listeners = new Set<() => void>()
const restartListeners = new Set<() => void>()

function getSnapshot(): DeviceStatus {
  return status
}

function subscribe(callback: () => void): () => void {
  listeners.add(callback)
  return () => listeners.delete(callback)
}

function emitChange(): void {
  listeners.forEach((l) => l())
  restartListeners.forEach((l) => l())
}

/** 设置设备状态。仅由 DeviceProvider 调用。 */
export function setDeviceStatus(connectionStatus: ConnectionStatus, activated: boolean | null): void {
  const next: DeviceStatus = { connectionStatus, activated, runtimeKind: status.runtimeKind }
  if (
    status.connectionStatus === next.connectionStatus &&
    status.activated === next.activated &&
    status.runtimeKind === next.runtimeKind
  ) {
    return
  }
  status = next
  emitChange()
}

/** 配置刷新成功后可调用：立即把连接态标记为可达，避免 UI 继续显示断连蒙层。 */
export function markDeviceReachable(): void {
  if (status.connectionStatus === 'reachable') return
  status = { ...status, connectionStatus: 'reachable', runtimeKind: status.runtimeKind }
  emitChange()
}

/** 重启指令已发出，由 TopBar 在 POST /api/restart 成功后调用。 */
export function setRestartPending(): void {
  restartPending = true
  restartPendingSince = Date.now()
  restartDropTime = null
  emitChange()
}

/**
 * 轮询得到新连接状态后调用，用于推进重启闭环：不可达时记 dropTime，再次可达则置「重启完成」并清 pending；
 * 自 drop 起超过 1 分钟仍不可达则置「设备可能异常」并清 pending。
 */
export function updateRestartState(connectionStatus: ConnectionStatus): void {
  if (!restartPending) return
  if (
    restartDropTime === null &&
    restartPendingSince !== null &&
    Date.now() - restartPendingSince > RESTART_PENDING_MAX_MS
  ) {
    restartPending = false
    restartPendingSince = null
    restartDropTime = null
    restartTimeout = true
    emitChange()
    return
  }
  if (connectionStatus === 'unreachable') {
    if (restartDropTime === null) restartDropTime = Date.now()
    else if (Date.now() - restartDropTime > RESTART_TIMEOUT_MS) {
      restartPending = false
      restartPendingSince = null
      restartDropTime = null
      restartTimeout = true
      emitChange()
    }
    return
  }
  if (connectionStatus === 'reachable' && restartDropTime !== null) {
    restartPending = false
    restartPendingSince = null
    restartDropTime = null
    reconnectedAfterRestart = true
    emitChange()
  }
}

/** 当前重启阶段，供横幅等展示。 */
export function getRestartPhase(): RestartPhase {
  if (!restartPending) return 'idle'
  return restartDropTime === null ? 'pending' : 'restarting'
}

/** 一次性：重启后已重新连接，消费后清除。用于弹 toast「重启完成」。 */
export function consumeReconnectedAfterRestart(): boolean {
  const v = reconnectedAfterRestart
  if (v) {
    reconnectedAfterRestart = false
    emitChange()
  }
  return v
}

/** 一次性：重启流程超时，消费后清除。用于弹 toast「设备可能异常」。 */
export function consumeRestartTimeout(): boolean {
  const v = restartTimeout
  if (v) {
    restartTimeout = false
    emitChange()
  }
  return v
}

function subscribeRestart(cb: () => void): () => void {
  restartListeners.add(cb)
  return () => restartListeners.delete(cb)
}
/** 订阅重启阶段变化（phase）；单次事件由上层用 consume* 消费。getSnapshot 必须返回稳定引用，否则会触发 React #185 无限重渲染。 */
export function useRestartPhase(): RestartPhase {
  return useSyncExternalStore(subscribeRestart, getRestartPhase, getRestartPhase)
}

/** 在组件中订阅完整设备状态。 */
export function useDeviceStatus(): DeviceStatus {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot)
}

/** 仅订阅运行平台（Linux / ESP / host / unknown），用于表单项显隐。 */
export function useDeviceRuntimeKind(): DeviceRuntimeKind {
  return useSyncExternalStore(subscribe, getRuntimeKindSnapshot, getRuntimeKindSnapshot)
}

/** 设备是否已连接（可达且已拿到 pairing_code 响应）。 */
export function useDeviceConnected(): boolean {
  return useDeviceStatus().connectionStatus === 'reachable'
}

/** 非 React 环境获取当前是否已连接。 */
export function getDeviceConnected(): boolean {
  return status.connectionStatus === 'reachable'
}
