import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { API_ERROR, type ApiResult } from "../api/client";
import type {
  ChannelsConfigView,
  LlmConfigSegment,
  ChannelsConfigSegment,
  SystemConfigSegment,
} from "../types/appConfig";
import { normalizeChannelsConfigFromDevice } from "../types/appConfig";
import type { DisplayConfig } from "../types/displayConfig";
import { normalizeDisplayConfig } from "../types/displayConfig";
import type { HardwareSegment } from "../types/hardwareConfig";
import type { AudioConfig } from "../types/audioConfig";
import { normalizeAudioConfigFromDevice } from "../types/audioConfig";
import { ensureHardwareDeviceIds } from "../util/hardwareDeviceId";
import { ConfigContext } from "./ConfigContext";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { useDevice } from "../hooks/useDevice";
import { isDeviceOrPairingErrorKey } from "../i18n/apiErrors";
import { fetchSystemInfoCoalesced } from "../session/systemInfoCoordinator";
import { markDeviceReachable } from "../store/deviceStatusStore";

/** i18n keys for config load errors; 与 shell blocker 重复的设备/配对类不在子页内重复展示 */
const ERROR_KEY_NO_BASE = "device.bannerNeedDevice";
const ERROR_KEY_LOAD_FAILED = "config.errorLoadFailed";

function mapSegmentLoadError(res: ApiResult<unknown>): string | null {
  return res.error === API_ERROR.NO_BASE_URL
    ? ERROR_KEY_NO_BASE
    : isDeviceOrPairingErrorKey(res.error ?? "")
      ? null
      : (res.error ?? ERROR_KEY_LOAD_FAILED);
}

function mapSaveError(error: string | undefined): string | undefined {
  return error === API_ERROR.PAIRING_REQUIRED
    ? "device.pairingCodeRequired"
    : error;
}

/** display / audio / hardware / system / 其他分段 GET 共用：ready、loading、错误映射一致。 */
async function loadDeviceSegment<T extends object>(args: {
  ready: boolean;
  setLoading: (v: boolean) => void;
  setError: (v: string | null) => void;
  fetch: () => Promise<ApiResult<T>>;
  applySuccess: (data: T) => void;
  clearData: () => void;
  isCurrent: () => boolean;
}): Promise<void> {
  const { ready, setLoading, setError, fetch, applySuccess, clearData, isCurrent } =
    args;
  if (!ready) {
    if (!isCurrent()) return;
    setLoading(false);
    setError(ERROR_KEY_NO_BASE);
    clearData();
    return;
  }
  setLoading(true);
  setError(null);
  const res = await fetch();
  if (!isCurrent()) return;
  setLoading(false);
  if (res.ok && res.data != null && typeof res.data === "object") {
    applySuccess(res.data as T);
  } else {
    setError(mapSegmentLoadError(res));
    clearData();
  }
}

export function ConfigProvider({ children }: { children: React.ReactNode }) {
  const { baseUrl, pairingCode } = useDevice();
  const { api, ready, deviceConnected } = useDeviceApi();
  const deviceSessionKey = `${baseUrl?.trim() ?? ""}\0${(pairingCode ?? "").trim()}`;
  const deviceSessionKeyRef = useRef(deviceSessionKey);
  const [systemConfig, setSystemConfig] = useState<SystemConfigSegment | null>(null);
  const [systemLoading, setSystemLoading] = useState(false);
  const [systemError, setSystemError] = useState<string | null>(null);
  const [llmConfig, setLlmConfig] = useState<LlmConfigSegment | null>(null);
  const [llmLoading, setLlmLoading] = useState(false);
  const [llmError, setLlmError] = useState<string | null>(null);
  const [channelsConfig, setChannelsConfig] = useState<ChannelsConfigView | null>(
    null,
  );
  const [channelsLoading, setChannelsLoading] = useState(false);
  const [channelsError, setChannelsError] = useState<string | null>(null);
  const [displayConfig, setDisplayConfig] = useState<DisplayConfig | null>(
    null,
  );
  const [displayLoading, setDisplayLoading] = useState(false);
  const [displayError, setDisplayError] = useState<string | null>(null);
  const [audioConfig, setAudioConfig] = useState<AudioConfig | null>(null);
  const [audioLoading, setAudioLoading] = useState(false);
  const [audioError, setAudioError] = useState<string | null>(null);
  const [hardwareSegment, setHardwareSegment] =
    useState<HardwareSegment | null>(null);
  const [hardwareLoading, setHardwareLoading] = useState(false);
  const [hardwareError, setHardwareError] = useState<string | null>(null);

  /** 未进「设备」页时也要能解析平台（显示页等按 Linux/ESP 裁剪 UI）；与 DevicePage 共用协调器避免重复请求。 */
  useEffect(() => {
    if (!ready || !deviceConnected || !baseUrl?.trim()) return;
    const code = (pairingCode ?? "").trim();
    void fetchSystemInfoCoalesced(baseUrl.trim(), code, () => api.system.info(), {
      force: false,
    });
  }, [ready, deviceConnected, baseUrl, pairingCode, api.system]);

  const clearCachedSystemConfig = useCallback(() => {
    setSystemConfig(null);
    setSystemLoading(false);
    setSystemError(null);
    setLlmConfig(null);
    setLlmLoading(false);
    setLlmError(null);
    setChannelsConfig(null);
    setChannelsLoading(false);
    setChannelsError(null);
    setDisplayConfig(null);
    setDisplayLoading(false);
    setDisplayError(null);
    setAudioConfig(null);
    setAudioLoading(false);
    setAudioError(null);
    setHardwareSegment(null);
    setHardwareLoading(false);
    setHardwareError(null);
  }, []);

  useEffect(() => {
    deviceSessionKeyRef.current = deviceSessionKey;
    let cancelled = false;
    queueMicrotask(() => {
      if (!cancelled) clearCachedSystemConfig();
    });
    return () => {
      cancelled = true;
    };
  }, [deviceSessionKey, clearCachedSystemConfig]);

  const loadSystemConfig = useCallback(async () => {
    const sessionKey = deviceSessionKey;
    await loadDeviceSegment({
      ready,
      setLoading: setSystemLoading,
      setError: setSystemError,
      fetch: () => api.config.getSystem() as Promise<ApiResult<SystemConfigSegment>>,
      applySuccess: (data) => setSystemConfig(data),
      clearData: () => setSystemConfig(null),
      isCurrent: () => deviceSessionKeyRef.current === sessionKey,
    });
  }, [api.config, deviceSessionKey, ready]);

  const loadLlmConfig = useCallback(async () => {
    const sessionKey = deviceSessionKey;
    await loadDeviceSegment({
      ready,
      setLoading: setLlmLoading,
      setError: setLlmError,
      fetch: () => api.config.getLlm() as Promise<ApiResult<LlmConfigSegment>>,
      applySuccess: (data) => setLlmConfig(data),
      clearData: () => setLlmConfig(null),
      isCurrent: () => deviceSessionKeyRef.current === sessionKey,
    });
  }, [api.config, deviceSessionKey, ready]);

  const loadChannelsConfig = useCallback(async () => {
    const sessionKey = deviceSessionKey;
    await loadDeviceSegment({
      ready,
      setLoading: setChannelsLoading,
      setError: setChannelsError,
      fetch: () =>
        api.config.getChannels() as Promise<ApiResult<ChannelsConfigView>>,
      applySuccess: (data) => setChannelsConfig(normalizeChannelsConfigFromDevice(data)),
      clearData: () => setChannelsConfig(null),
      isCurrent: () => deviceSessionKeyRef.current === sessionKey,
    });
  }, [api.config, deviceSessionKey, ready]);

  /**
   * 在“断连但有缓存”场景下尝试刷新：成功则更新缓存，失败保留现有缓存不清空。
   */
  const refreshCachedSystemConfig = useCallback(async (): Promise<{
    ok: boolean;
    error?: string;
  }> => {
    const sessionKey = deviceSessionKey;
    if (!ready) return { ok: false, error: ERROR_KEY_NO_BASE };
    const res = await api.config.getSystem();
    if (deviceSessionKeyRef.current !== sessionKey) {
      return { ok: false, error: undefined };
    }
    if (res.ok && res.data != null && typeof res.data === "object") {
      setSystemConfig(res.data as SystemConfigSegment);
      setSystemError(null);
      markDeviceReachable();
      return { ok: true };
    }
    return { ok: false, error: res.error ?? ERROR_KEY_LOAD_FAILED };
  }, [api.config, deviceSessionKey, ready]);

  const saveSegment = useCallback(
    async <TBody extends object,>(
      save: (body: TBody) => Promise<ApiResult<unknown>>,
      body: TBody,
      applySuccess?: () => void,
    ): Promise<{ ok: boolean; error?: string }> => {
      const sessionKey = deviceSessionKey;
      const res = await save(body);
      if (res.ok && deviceSessionKeyRef.current === sessionKey) {
        applySuccess?.();
      }
      return { ok: res.ok ?? false, error: mapSaveError(res.error) };
    },
    [deviceSessionKey],
  );

  const saveLlm = useCallback(
    async (
      body: LlmConfigSegment,
    ): Promise<{ ok: boolean; error?: string }> => {
      return saveSegment(api.config.saveLlm, body, () => setLlmConfig(body));
    },
    [api.config.saveLlm, saveSegment],
  );

  const saveChannels = useCallback(
    async (
      body: ChannelsConfigSegment,
    ): Promise<{ ok: boolean; error?: string }> => {
      return saveSegment(api.config.saveChannels, body, () => {
        setChannelsConfig((prev) => {
          if (!prev) return prev;
          return {
            ...prev,
            ...body,
            unavailable_enabled_channel: undefined,
          };
        });
      });
    },
    [api.config.saveChannels, saveSegment],
  );

  const saveSystem = useCallback(
    async (
      body: SystemConfigSegment,
    ): Promise<{ ok: boolean; error?: string }> => {
      return saveSegment(api.config.saveSystem, body, () => {
        setSystemConfig((prev) => (prev ? { ...prev, ...body } : body));
      });
    },
    [api.config.saveSystem, saveSegment],
  );

  const loadDisplayConfig = useCallback(async () => {
    const sessionKey = deviceSessionKey;
    await loadDeviceSegment({
      ready,
      setLoading: setDisplayLoading,
      setError: setDisplayError,
      fetch: () => api.display.get() as Promise<ApiResult<DisplayConfig>>,
      applySuccess: (data) =>
        setDisplayConfig(
          normalizeDisplayConfig(data as Partial<DisplayConfig> & Record<string, unknown>),
        ),
      clearData: () => setDisplayConfig(null),
      isCurrent: () => deviceSessionKeyRef.current === sessionKey,
    });
  }, [api.display, deviceSessionKey, ready]);

  const saveDisplayConfig = useCallback(
    async (
      body: DisplayConfig,
    ): Promise<{ ok: boolean; error?: string; restartRequired?: boolean }> => {
      const sessionKey = deviceSessionKey;
      const res = await api.display.save(body);
      if (res.ok && deviceSessionKeyRef.current === sessionKey) {
        setDisplayConfig(body);
      }
      const err =
        res.error === API_ERROR.PAIRING_REQUIRED
          ? "device.pairingCodeRequired"
          : res.error;
      return {
        ok: res.ok ?? false,
        error: err,
        restartRequired: Boolean(res.ok && res.data?.restart_required),
      };
    },
    [api.display, deviceSessionKey],
  );

  const loadAudioConfig = useCallback(async () => {
    const sessionKey = deviceSessionKey;
    await loadDeviceSegment({
      ready,
      setLoading: setAudioLoading,
      setError: setAudioError,
      fetch: () => api.audio.get() as Promise<ApiResult<AudioConfig>>,
      applySuccess: (data) => setAudioConfig(normalizeAudioConfigFromDevice(data)),
      clearData: () => setAudioConfig(null),
      isCurrent: () => deviceSessionKeyRef.current === sessionKey,
    });
  }, [api.audio, deviceSessionKey, ready]);

  const saveAudioConfig = useCallback(
    async (
      body: AudioConfig,
    ): Promise<{ ok: boolean; error?: string; restartRequired?: boolean }> => {
      const sessionKey = deviceSessionKey;
      const res = await api.audio.save(body);
      if (res.ok && deviceSessionKeyRef.current === sessionKey) {
        setAudioConfig(body);
      }
      const err =
        res.error === API_ERROR.PAIRING_REQUIRED
          ? "device.pairingCodeRequired"
          : res.error;
      return {
        ok: res.ok ?? false,
        error: err,
        restartRequired: Boolean(res.ok && res.data?.restart_required),
      };
    },
    [api.audio, deviceSessionKey],
  );

  const loadHardwareConfig = useCallback(async () => {
    const sessionKey = deviceSessionKey;
    await loadDeviceSegment({
      ready,
      setLoading: setHardwareLoading,
      setError: setHardwareError,
      fetch: () => api.hardware.get() as Promise<ApiResult<HardwareSegment>>,
      applySuccess: (data) => {
        const d = data as HardwareSegment;
        const list = Array.isArray(d.hardware_devices) ? d.hardware_devices : [];
        setHardwareSegment({
          hardware_devices: ensureHardwareDeviceIds(list),
          i2c_bus: d.i2c_bus ?? undefined,
          i2c_devices: Array.isArray(d.i2c_devices) ? d.i2c_devices : undefined,
          i2c_sensors: Array.isArray(d.i2c_sensors) ? d.i2c_sensors : [],
        });
      },
      clearData: () => setHardwareSegment(null),
      isCurrent: () => deviceSessionKeyRef.current === sessionKey,
    });
  }, [api.hardware, deviceSessionKey, ready]);

  const saveHardwareConfig = useCallback(
    async (
      body: HardwareSegment,
    ): Promise<{ ok: boolean; error?: string; restartRequired?: boolean }> => {
      const sessionKey = deviceSessionKey;
      const res = await api.hardware.save(body);
      if (res.ok && deviceSessionKeyRef.current === sessionKey) {
        setHardwareSegment(body);
      }
      const err =
        res.error === API_ERROR.PAIRING_REQUIRED
          ? "device.pairingCodeRequired"
          : res.error;
      return {
        ok: res.ok ?? false,
        error: err,
        restartRequired: Boolean(res.ok),
      };
    },
    [api.hardware, deviceSessionKey],
  );

  const value = useMemo(
    () => ({
      systemConfig,
      systemLoading,
      systemError,
      loadSystemConfig,
      llmConfig,
      llmLoading,
      llmError,
      loadLlmConfig,
      channelsConfig,
      channelsLoading,
      channelsError,
      loadChannelsConfig,
      refreshCachedSystemConfig,
      clearCachedSystemConfig,
      saveLlm,
      saveChannels,
      saveSystem,
      displayConfig,
      displayLoading,
      displayError,
      loadDisplayConfig,
      saveDisplayConfig,
      audioConfig,
      audioLoading,
      audioError,
      loadAudioConfig,
      saveAudioConfig,
      hardwareSegment,
      hardwareLoading,
      hardwareError,
      loadHardwareConfig,
      saveHardwareConfig,
    }),
    [
      systemConfig,
      systemLoading,
      systemError,
      loadSystemConfig,
      llmConfig,
      llmLoading,
      llmError,
      loadLlmConfig,
      channelsConfig,
      channelsLoading,
      channelsError,
      loadChannelsConfig,
      refreshCachedSystemConfig,
      clearCachedSystemConfig,
      saveLlm,
      saveChannels,
      saveSystem,
      displayConfig,
      displayLoading,
      displayError,
      loadDisplayConfig,
      saveDisplayConfig,
      audioConfig,
      audioLoading,
      audioError,
      loadAudioConfig,
      saveAudioConfig,
      hardwareSegment,
      hardwareLoading,
      hardwareError,
      loadHardwareConfig,
      saveHardwareConfig,
    ],
  );

  return (
    <ConfigContext.Provider value={value}>{children}</ConfigContext.Provider>
  );
}
