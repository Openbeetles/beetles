import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { API_ERROR, type ApiResult } from "../api/client";
import type {
  AppConfig,
  LlmConfigSegment,
  ChannelsConfigSegment,
  SystemConfigSegment,
} from "../types/appConfig";
import { llmConfigSegmentFromAppConfig } from "../types/appConfig";
import type { DisplayConfig } from "../types/displayConfig";
import { normalizeDisplayConfig } from "../types/displayConfig";
import type { HardwareSegment } from "../types/hardwareConfig";
import type { AudioConfig } from "../types/audioConfig";
import { normalizeAudioConfigFromDevice } from "../types/audioConfig";
import { ensureHardwareDeviceIds } from "../util/hardwareDeviceId";
import { ConfigContext } from "./ConfigContext";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { useDevice } from "../hooks/useDevice";
import { fetchSystemInfoCoalesced } from "../session/systemInfoCoordinator";
import { markDeviceReachable } from "../store/deviceStatusStore";

/** i18n keys for config load errors; 与顶栏横幅重复的配对/设备类不展示，由 DeviceBanner 处理 */
const ERROR_KEY_NO_BASE = "device.bannerNeedDevice";
const ERROR_KEY_LOAD_FAILED = "config.errorLoadFailed";

/** API 返回的原始配对/设备类文案，子页不重复展示 */
function isDeviceOrPairingHint(err: string | undefined): boolean {
  if (!err) return false;
  const s = err.trim();
  return (
    s === "请先设置配对码" ||
    s === "请先填写设备地址" ||
    s === "配对码错误" ||
    s === "Please set pairing code first" ||
    s === "Please enter device URL" ||
    s === "Wrong pairing code"
  );
}

function mapSegmentLoadError(res: ApiResult<unknown>): string | null {
  return res.error === API_ERROR.NO_BASE_URL
    ? ERROR_KEY_NO_BASE
    : isDeviceOrPairingHint(res.error ?? "")
      ? null
      : ERROR_KEY_LOAD_FAILED;
}

function mapSaveError(error: string | undefined): string | undefined {
  return error === API_ERROR.PAIRING_REQUIRED
    ? "device.pairingCodeRequired"
    : error;
}

/** display / audio / hardware / 主配置 GET 共用：ready、loading、错误映射一致。 */
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
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [llmConfig, setLlmConfig] = useState<LlmConfigSegment | null>(null);
  const [llmLoading, setLlmLoading] = useState(false);
  const [llmError, setLlmError] = useState<string | null>(null);
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

  const clearCachedConfig = useCallback(() => {
    setConfig(null);
    setLoading(false);
    setError(null);
    setLlmConfig(null);
    setLlmLoading(false);
    setLlmError(null);
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
      if (!cancelled) clearCachedConfig();
    });
    return () => {
      cancelled = true;
    };
  }, [deviceSessionKey, clearCachedConfig]);

  const loadConfig = useCallback(async () => {
    const sessionKey = deviceSessionKey;
    await loadDeviceSegment({
      ready,
      setLoading,
      setError,
      fetch: () => api.config.get() as Promise<ApiResult<AppConfig>>,
      applySuccess: (data) => {
        setConfig(data);
        setLlmConfig(llmConfigSegmentFromAppConfig(data));
      },
      clearData: () => {
        setConfig(null);
        setLlmConfig(null);
      },
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

  /**
   * 在“断连但有缓存”场景下尝试刷新：成功则更新缓存，失败保留现有缓存不清空。
   */
  const refreshCachedConfig = useCallback(async (): Promise<{
    ok: boolean;
    error?: string;
  }> => {
    const sessionKey = deviceSessionKey;
    if (!ready) return { ok: false, error: ERROR_KEY_NO_BASE };
    const res = await api.config.get();
    if (deviceSessionKeyRef.current !== sessionKey) {
      return { ok: false, error: undefined };
    }
    if (res.ok && res.data != null && typeof res.data === "object") {
      const data = res.data as AppConfig;
      setConfig(data);
      setLlmConfig(llmConfigSegmentFromAppConfig(data));
      setError(null);
      markDeviceReachable();
      return { ok: true };
    }
    return { ok: false, error: res.error ?? ERROR_KEY_LOAD_FAILED };
  }, [api.config, deviceSessionKey, ready]);

  const saveConfigSegment = useCallback(
    async <TBody extends object,>(
      save: (body: TBody) => Promise<ApiResult<unknown>>,
      body: TBody,
      applySuccess?: (body: TBody) => void,
    ): Promise<{ ok: boolean; error?: string }> => {
      const sessionKey = deviceSessionKey;
      const res = await save(body);
      if (res.ok && deviceSessionKeyRef.current === sessionKey) {
        applySuccess?.(body);
        setConfig((prev) => (prev ? { ...prev, ...body } : null));
      }
      return { ok: res.ok ?? false, error: mapSaveError(res.error) };
    },
    [deviceSessionKey],
  );

  const saveLlm = useCallback(
    async (
      body: LlmConfigSegment,
    ): Promise<{ ok: boolean; error?: string }> => {
      return saveConfigSegment(api.config.saveLlm, body, setLlmConfig);
    },
    [api.config.saveLlm, saveConfigSegment],
  );

  const saveChannels = useCallback(
    async (
      body: ChannelsConfigSegment,
    ): Promise<{ ok: boolean; error?: string }> => {
      return saveConfigSegment(api.config.saveChannels, body);
    },
    [api.config.saveChannels, saveConfigSegment],
  );

  const saveSystem = useCallback(
    async (
      body: SystemConfigSegment,
    ): Promise<{ ok: boolean; error?: string }> => {
      return saveConfigSegment(api.config.saveSystem, body);
    },
    [api.config.saveSystem, saveConfigSegment],
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
      config,
      loading,
      error,
      loadConfig,
      llmConfig,
      llmLoading,
      llmError,
      loadLlmConfig,
      refreshCachedConfig,
      clearCachedConfig,
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
      config,
      loading,
      error,
      loadConfig,
      llmConfig,
      llmLoading,
      llmError,
      loadLlmConfig,
      refreshCachedConfig,
      clearCachedConfig,
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
