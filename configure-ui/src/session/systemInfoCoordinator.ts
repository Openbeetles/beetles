/**
 * GET /api/system_info 会话级合并：并发共享同一 Promise、TTL 内复用结果，减轻固件压力。
 * Session-level coalescing for system_info: shared in-flight promise + TTL cache.
 */
import type { ApiResult } from "../api/client";
import { API_ERROR } from "../api/client";
import type { SystemInfoData } from "../api/endpoints/system";
import { setDeviceRuntimeKindFromBoardId } from "../store/deviceStatusStore";

export const SYSTEM_INFO_TTL_MS = 60_000;

function makeSessionKey(baseUrl: string, pairingCode: string): string {
  return `${baseUrl.trim()}\0${pairingCode.trim()}`;
}

let activeKey = "";
let cached: { data: SystemInfoData; at: number } | null = null;
let inFlight: Promise<ApiResult<SystemInfoData>> | null = null;
let inFlightKey = "";

/** baseUrl / 配对码变化时清空缓存与进行中的合并（由 DeviceProvider 调用）。 */
export function resetSystemInfoCache(): void {
  activeKey = "";
  cached = null;
  inFlight = null;
  inFlightKey = "";
}

/**
 * @param force 为 true 时忽略 TTL 强制走网络；仍可与其它并发调用共享同一 in-flight。
 */
export async function fetchSystemInfoCoalesced(
  baseUrl: string,
  pairingCode: string,
  infoFn: () => Promise<ApiResult<SystemInfoData>>,
  options?: { force?: boolean },
): Promise<ApiResult<SystemInfoData>> {
  const trimmedBase = baseUrl.trim();
  if (!trimmedBase) {
    return { ok: false, error: API_ERROR.NO_BASE_URL };
  }

  const key = makeSessionKey(trimmedBase, pairingCode);
  if (activeKey !== key) {
    activeKey = key;
    cached = null;
    inFlight = null;
    inFlightKey = "";
  }

  const force = options?.force ?? false;
  const now = Date.now();
  if (!force && cached && now - cached.at < SYSTEM_INFO_TTL_MS) {
    return { ok: true, data: cached.data };
  }

  if (inFlight && inFlightKey === key) {
    return inFlight;
  }

  const promise = (async (): Promise<ApiResult<SystemInfoData>> => {
    try {
      const res = await infoFn();
      if (activeKey === key && res.ok && res.data) {
        cached = { data: res.data, at: Date.now() };
        setDeviceRuntimeKindFromBoardId(res.data.board_id);
      }
      return res;
    } finally {
      if (inFlightKey === key) {
        inFlight = null;
        inFlightKey = "";
      }
    }
  })();

  inFlight = promise;
  inFlightKey = key;
  return promise;
}
