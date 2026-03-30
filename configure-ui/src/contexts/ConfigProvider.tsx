import { useCallback, useEffect, useMemo, useState } from "react";
import { API_ERROR } from "../api/client";
import type {
  AppConfig,
  LlmConfigSegment,
  ChannelsConfigSegment,
  SystemConfigSegment,
} from "../types/appConfig";
import type { DisplayConfig } from "../types/displayConfig";
import { normalizeDisplayConfig } from "../types/displayConfig";
import type { HardwareSegment } from "../types/hardwareConfig";
import type { AudioConfig } from "../types/audioConfig";
import { ensureHardwareDeviceIds } from "../util/hardwareDeviceId";
import { ConfigContext } from "./ConfigContext";
import { useDeviceApi } from "../hooks/useDeviceApi";
import {
  markDeviceReachable,
  setDeviceRuntimeKindFromBoardId,
} from "../store/deviceStatusStore";

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

export function ConfigProvider({ children }: { children: React.ReactNode }) {
  const { api, ready, deviceConnected } = useDeviceApi();
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
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

  /** 未进「设备」页时也要能解析平台（显示页等按 Linux/ESP 裁剪 UI）。 */
  useEffect(() => {
    if (!ready || !deviceConnected) return;
    let cancelled = false;
    void api.system.info().then((res) => {
      if (cancelled || !res.ok || !res.data) return;
      setDeviceRuntimeKindFromBoardId(res.data.board_id);
    });
    return () => {
      cancelled = true;
    };
  }, [ready, deviceConnected, api.system]);

  const clearCachedConfig = useCallback(() => {
    setConfig(null);
    setError(null);
    setDisplayConfig(null);
    setDisplayError(null);
    setAudioConfig(null);
    setAudioError(null);
    setHardwareSegment(null);
    setHardwareError(null);
  }, []);

  const loadConfig = useCallback(async () => {
    if (!ready) {
      setError(ERROR_KEY_NO_BASE);
      return;
    }
    setLoading(true);
    setError(null);
    const res = await api.config.get();
    setLoading(false);
    if (res.ok && res.data != null && typeof res.data === "object") {
      setConfig(res.data as AppConfig);
    } else {
      setError(
        res.error === API_ERROR.NO_BASE_URL
          ? ERROR_KEY_NO_BASE
          : isDeviceOrPairingHint(res.error ?? "")
            ? null
            : ERROR_KEY_LOAD_FAILED,
      );
      setConfig(null);
    }
  }, [api.config, ready]);

  /**
   * 在“断连但有缓存”场景下尝试刷新：成功则更新缓存，失败保留现有缓存不清空。
   */
  const refreshCachedConfig = useCallback(async (): Promise<{
    ok: boolean;
    error?: string;
  }> => {
    if (!ready) return { ok: false, error: ERROR_KEY_NO_BASE };
    const res = await api.config.get();
    if (res.ok && res.data != null && typeof res.data === "object") {
      setConfig(res.data as AppConfig);
      setError(null);
      markDeviceReachable();
      return { ok: true };
    }
    return { ok: false, error: res.error ?? ERROR_KEY_LOAD_FAILED };
  }, [api.config, ready]);

  const saveLlm = useCallback(
    async (
      body: LlmConfigSegment,
    ): Promise<{ ok: boolean; error?: string }> => {
      const res = await api.config.saveLlm(body);
      if (res.ok) setConfig((prev) => (prev ? { ...prev, ...body } : null));
      const err =
        res.error === API_ERROR.PAIRING_REQUIRED
          ? "device.pairingCodeRequired"
          : res.error;
      return { ok: res.ok ?? false, error: err };
    },
    [api.config],
  );

  const saveChannels = useCallback(
    async (
      body: ChannelsConfigSegment,
    ): Promise<{ ok: boolean; error?: string }> => {
      const res = await api.config.saveChannels(body);
      if (res.ok) setConfig((prev) => (prev ? { ...prev, ...body } : null));
      const err =
        res.error === API_ERROR.PAIRING_REQUIRED
          ? "device.pairingCodeRequired"
          : res.error;
      return { ok: res.ok ?? false, error: err };
    },
    [api.config],
  );

  const saveSystem = useCallback(
    async (
      body: SystemConfigSegment,
    ): Promise<{ ok: boolean; error?: string }> => {
      const res = await api.config.saveSystem(body);
      if (res.ok) setConfig((prev) => (prev ? { ...prev, ...body } : null));
      const err =
        res.error === API_ERROR.PAIRING_REQUIRED
          ? "device.pairingCodeRequired"
          : res.error;
      return { ok: res.ok ?? false, error: err };
    },
    [api.config],
  );

  const loadDisplayConfig = useCallback(async () => {
    if (!ready) {
      setDisplayError(ERROR_KEY_NO_BASE);
      return;
    }
    setDisplayLoading(true);
    setDisplayError(null);
    const res = await api.display.get();
    setDisplayLoading(false);
    if (res.ok && res.data != null && typeof res.data === "object") {
      setDisplayConfig(
        normalizeDisplayConfig(res.data as Partial<DisplayConfig> & Record<string, unknown>),
      );
    } else {
      setDisplayError(
        res.error === API_ERROR.NO_BASE_URL
          ? ERROR_KEY_NO_BASE
          : isDeviceOrPairingHint(res.error ?? "")
            ? null
            : ERROR_KEY_LOAD_FAILED,
      );
      setDisplayConfig(null);
    }
  }, [api.display, ready]);

  const saveDisplayConfig = useCallback(
    async (
      body: DisplayConfig,
    ): Promise<{ ok: boolean; error?: string; restartRequired?: boolean }> => {
      const res = await api.display.save(body);
      if (res.ok) setDisplayConfig(body);
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
    [api.display],
  );

  const loadAudioConfig = useCallback(async () => {
    if (!ready) {
      setAudioError(ERROR_KEY_NO_BASE);
      return;
    }
    setAudioLoading(true);
    setAudioError(null);
    const res = await api.audio.get();
    setAudioLoading(false);
    if (res.ok && res.data != null && typeof res.data === "object") {
      setAudioConfig(res.data as AudioConfig);
    } else {
      setAudioError(
        res.error === API_ERROR.NO_BASE_URL
          ? ERROR_KEY_NO_BASE
          : isDeviceOrPairingHint(res.error ?? "")
            ? null
            : ERROR_KEY_LOAD_FAILED,
      );
      setAudioConfig(null);
    }
  }, [api.audio, ready]);

  const saveAudioConfig = useCallback(
    async (
      body: AudioConfig,
    ): Promise<{ ok: boolean; error?: string; restartRequired?: boolean }> => {
      const res = await api.audio.save(body);
      if (res.ok) setAudioConfig(body);
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
    [api.audio],
  );

  const loadHardwareConfig = useCallback(async () => {
    if (!ready) {
      setHardwareError(ERROR_KEY_NO_BASE);
      return;
    }
    setHardwareLoading(true);
    setHardwareError(null);
    const res = await api.hardware.get();
    setHardwareLoading(false);
    if (res.ok && res.data != null && typeof res.data === "object") {
      const d = res.data as HardwareSegment;
      const list = Array.isArray(d.hardware_devices) ? d.hardware_devices : [];
      setHardwareSegment({
        hardware_devices: ensureHardwareDeviceIds(list),
        i2c_bus: d.i2c_bus ?? undefined,
        i2c_devices: Array.isArray(d.i2c_devices) ? d.i2c_devices : undefined,
        i2c_sensors: Array.isArray(d.i2c_sensors) ? d.i2c_sensors : [],
      });
    } else {
      setHardwareError(
        res.error === API_ERROR.NO_BASE_URL
          ? ERROR_KEY_NO_BASE
          : isDeviceOrPairingHint(res.error ?? "")
            ? null
            : ERROR_KEY_LOAD_FAILED,
      );
      setHardwareSegment(null);
    }
  }, [api.hardware, ready]);

  const saveHardwareConfig = useCallback(
    async (
      body: HardwareSegment,
    ): Promise<{ ok: boolean; error?: string; restartRequired?: boolean }> => {
      const res = await api.hardware.save(body);
      if (res.ok) setHardwareSegment(body);
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
    [api.hardware],
  );

  const value = useMemo(
    () => ({
      config,
      loading,
      error,
      loadConfig,
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
