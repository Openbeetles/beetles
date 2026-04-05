import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import SaveRounded from "@mui/icons-material/SaveRounded";
import RouterRounded from "@mui/icons-material/RouterRounded";
import ChatRounded from "@mui/icons-material/ChatRounded";
import InfoOutlined from "@mui/icons-material/InfoOutlined";
import RefreshRounded from "@mui/icons-material/RefreshRounded";
import { InlineAlert, SaveFeedback } from "../components/form";
import { ChannelConnectivityPanel } from "../components/ChannelConnectivityPanel";
import { SettingsSection } from "../components/SettingsSection";
import { BeetleIcon } from "../components/BeetleIcon";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { useDevice } from "../hooks/useDevice";
import {
  type SystemInfoData,
  type ChannelConnectivityItem,
  type HealthData,
} from "../api/endpoints/system";
import { fetchSystemInfoCoalesced } from "../session/systemInfoCoordinator";
import { SystemStatusPanel } from "../components/SystemStatusPanel";
import { SectionLoadProgress } from "../components/SectionLoadProgress";
import { useUnsaved } from "../hooks/useUnsaved";

const DEFAULT_DEVICE_BASE_URL = "http://192.168.4.1";

function normalizeDeviceUrl(u: string): string {
  return u.trim().replace(/\/$/, "") || DEFAULT_DEVICE_BASE_URL;
}

export function DevicePage() {
  const { t } = useTranslation();
  const { setDirty } = useUnsaved();
  const { baseUrl, pairingCode, setBaseUrl, setPairingCode } = useDevice();
  const { api, deviceConnected } = useDeviceApi();
  const [urlInput, setUrlInput] = useState(baseUrl || DEFAULT_DEVICE_BASE_URL);
  const [codeInput, setCodeInput] = useState(pairingCode);
  const [probeStatus, setProbeStatus] = useState<
    "idle" | "checking" | "ok" | "fail"
  >("idle");
  const [probeError, setProbeError] = useState("");
  const [systemInfo, setSystemInfo] = useState<SystemInfoData | null>(null);
  const [systemInfoLoading, setSystemInfoLoading] = useState(false);
  const [systemInfoError, setSystemInfoError] = useState("");
  const [channelList, setChannelList] = useState<ChannelConnectivityItem[]>([]);
  const [channelLoading, setChannelLoading] = useState(false);
  const [channelError, setChannelError] = useState("");
  const [healthData, setHealthData] = useState<HealthData | null>(null);
  const [healthLoading, setHealthLoading] = useState(false);
  const [healthError, setHealthError] = useState("");
  const [saveStatus, setSaveStatus] = useState<"idle" | "ok">("idle");

  /** 设备上下文（地址/配对码）会话变化时同步输入框，避免与其它入口写入脱节 */
  const deviceSessionKey = `${baseUrl ?? ""}\0${pairingCode ?? ""}`;
  const prevDeviceSessionKeyRef = useRef(deviceSessionKey);
  useEffect(() => {
    if (deviceSessionKey === prevDeviceSessionKeyRef.current) return;
    prevDeviceSessionKeyRef.current = deviceSessionKey;
    const nextUrl = baseUrl || DEFAULT_DEVICE_BASE_URL;
    const nextCode = pairingCode ?? "";
    queueMicrotask(() => {
      setUrlInput(nextUrl);
      setCodeInput(nextCode);
    });
  }, [deviceSessionKey, baseUrl, pairingCode]);

  const connectionDraftDirty = useMemo(() => {
    const draftUrl = normalizeDeviceUrl(urlInput);
    const savedUrl = normalizeDeviceUrl(baseUrl ?? "");
    const draftCode = (codeInput ?? "").trim();
    const savedCode = (pairingCode ?? "").trim();
    return draftUrl !== savedUrl || draftCode !== savedCode;
  }, [urlInput, codeInput, baseUrl, pairingCode]);

  useEffect(() => {
    setDirty(connectionDraftDirty);
  }, [connectionDraftDirty, setDirty]);

  useEffect(() => {
    return () => setDirty(false);
  }, [setDirty]);

  const handleSave = () => {
    const url = urlInput.trim().replace(/\/$/, "") || DEFAULT_DEVICE_BASE_URL;
    setBaseUrl(url);
    setPairingCode(codeInput.trim());
    setSaveStatus("ok");
  };

  const handleProbe = async () => {
    const url = urlInput.trim().replace(/\/$/, "") || DEFAULT_DEVICE_BASE_URL;
    setProbeStatus("checking");
    setProbeError("");
    const res = await api.device.probe(url);
    if (res.ok) {
      setProbeStatus("ok");
    } else {
      setProbeStatus("fail");
      setProbeError(res.error ?? "");
    }
  };

  useEffect(() => {
    if (!deviceConnected || !baseUrl?.trim()) return;
    let cancelled = false;
    const tid = window.setTimeout(() => {
      if (!cancelled) {
        setSystemInfoLoading(true);
        setSystemInfoError("");
      }
    }, 0);
    const code = (pairingCode ?? "").trim();
    void fetchSystemInfoCoalesced(baseUrl.trim(), code, () => api.system.info(), {
      force: false,
    })
      .then((res) => {
        if (cancelled) return;
        setSystemInfoLoading(false);
        if (res.ok && res.data) {
          setSystemInfo(res.data);
        } else setSystemInfoError(res.error ?? "");
      })
      .catch(() => {
        if (!cancelled) {
          setSystemInfoLoading(false);
          setSystemInfoError("config.errorNetwork");
        }
      });
    return () => {
      cancelled = true;
      window.clearTimeout(tid);
    };
  }, [api.system, deviceConnected, baseUrl, pairingCode]);

  useEffect(() => {
    if (!deviceConnected || !baseUrl?.trim()) return;
    let cancelled = false;
    const tid = window.setTimeout(() => {
      if (!cancelled) {
        setHealthLoading(true);
        setHealthError("");
      }
    }, 0);
    api.system
      .health()
      .then((res) => {
        if (cancelled) return;
        setHealthLoading(false);
        if (res.ok && res.data) setHealthData(res.data);
        else setHealthError(res.error ?? "");
      })
      .catch(() => {
        if (!cancelled) {
          setHealthLoading(false);
          setHealthError("config.errorNetwork");
        }
      });
    return () => {
      cancelled = true;
      window.clearTimeout(tid);
    };
  }, [api.system, deviceConnected, baseUrl]);

  useEffect(() => {
    if (!deviceConnected || !baseUrl?.trim()) return;
    let cancelled = false;
    const tid = window.setTimeout(() => {
      if (!cancelled) {
        setChannelLoading(true);
        setChannelError("");
      }
    }, 0);
    api.system
      .channelConnectivity()
      .then((res) => {
        if (cancelled) return;
        setChannelLoading(false);
        if (res.ok && res.data?.channels) setChannelList(res.data.channels);
        else setChannelError(res.error ?? "channel connectivity unavailable");
      })
      .catch(() => {
        if (!cancelled) {
          setChannelLoading(false);
          setChannelError("config.errorNetwork");
        }
      });
    return () => {
      cancelled = true;
      window.clearTimeout(tid);
    };
  }, [api.system, deviceConnected, baseUrl]);

  const reloadSystemInfo = () => {
    setSystemInfo(null);
    setSystemInfoError("");
    if (!deviceConnected || !baseUrl?.trim()) return;
    setSystemInfoLoading(true);
    const code = (pairingCode ?? "").trim();
    void fetchSystemInfoCoalesced(baseUrl.trim(), code, () => api.system.info(), {
      force: true,
    })
      .then((res) => {
        setSystemInfoLoading(false);
        if (res.ok && res.data) {
          setSystemInfo(res.data);
        } else setSystemInfoError(res.error ?? "");
      })
      .catch(() => {
        setSystemInfoLoading(false);
        setSystemInfoError("config.errorNetwork");
      });
  };

  const reloadChannelConnectivity = () => {
    setChannelError("");
    if (!deviceConnected || !baseUrl?.trim()) return;
    setChannelLoading(true);
    api.system
      .channelConnectivity()
      .then((res) => {
        setChannelLoading(false);
        if (res.ok && res.data?.channels) setChannelList(res.data.channels);
        else setChannelError(res.error ?? "channel connectivity unavailable");
      })
      .catch(() => {
        setChannelLoading(false);
        setChannelError("config.errorNetwork");
      });
  };

  const reloadHealth = () => {
    setHealthError("");
    if (!deviceConnected || !baseUrl?.trim()) return;
    setHealthLoading(true);
    api.system
      .health()
      .then((res) => {
        setHealthLoading(false);
        if (res.ok && res.data) setHealthData(res.data);
        else setHealthError(res.error ?? "");
      })
      .catch(() => {
        setHealthLoading(false);
        setHealthError("config.errorNetwork");
      });
  };

  const channelNameKey: Record<string, string> = {
    telegram: "channelTelegram",
    feishu: "channelFeishu",
    dingtalk: "channelDingtalk",
    wecom: "channelWecom",
    qq_channel: "channelQqChannel",
    webhook: "channelWebhook",
  };

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 4 }}>
      <InlineAlert
        message={
          probeStatus === "fail" ? `${t("device.probeFail")}: ${probeError}` : null
        }
        onRetry={handleProbe}
      />
      <SettingsSection
        icon={<RouterRounded sx={{ fontSize: "var(--icon-size-md)" }} />}
        label={t("device.sectionConnection")}
        description={t("device.sectionConnectionDesc")}
      >
        <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
          <TextField
            label={t("device.baseUrlLabel")}
            placeholder={t("device.baseUrlPlaceholder")}
            value={urlInput}
            onChange={(e) => {
              setUrlInput(e.target.value);
              setSaveStatus("idle");
            }}
            size="small"
            fullWidth
            slotProps={{ htmlInput: { style: { fontFamily: "var(--font-mono)" } } }}
          />
          <TextField
            label={t("device.pairingCodeLabel")}
            placeholder={t("device.pairingCodePlaceholder")}
            value={codeInput}
            onChange={(e) => {
              setCodeInput(e.target.value);
              setSaveStatus("idle");
            }}
            size="small"
            fullWidth
            slotProps={{
              htmlInput: { maxLength: 6, style: { fontFamily: "var(--font-mono)" } },
            }}
          />
          <Box sx={{ display: "flex", gap: 1, flexWrap: "wrap" }}>
            <Button
              variant="contained"
              size="small"
              startIcon={<SaveRounded />}
              onClick={handleSave}
              sx={{ borderRadius: "var(--radius-control)" }}
            >
              {t("device.save")}
            </Button>
            <Button
              variant="outlined"
              size="small"
              onClick={handleProbe}
              disabled={probeStatus === "checking"}
              sx={{ borderRadius: "var(--radius-control)" }}
            >
              {probeStatus === "checking"
                ? t("device.probing")
                : t("device.probe")}
            </Button>
          </Box>
          {probeStatus === "ok" && (
            <Typography variant="caption" sx={{ color: "var(--semantic-success)" }}>
              {t("device.probeOk")}
            </Typography>
          )}
          {saveStatus === "ok" && (
            <SaveFeedback
              status="ok"
              message={t("common.saveOk")}
              autoDismissMs={3000}
              onDismiss={() => setSaveStatus("idle")}
            />
          )}
        </Box>
      </SettingsSection>

      {deviceConnected && (
        <Box
          sx={{
            display: "grid",
            gridTemplateColumns: { xs: "1fr", lg: "repeat(4, minmax(0, 1fr))" },
            gap: 2,
            alignItems: "stretch",
          }}
        >
          <Box sx={{ gridColumn: { xs: "span 1", lg: "span 1" }, minWidth: 0 }}>
            <SettingsSection
              icon={<ChatRounded sx={{ fontSize: "var(--icon-size-md)" }} />}
              label={t("device.sectionChannelConnectivity")}
              accessory={
                <Button
                  variant="outlined"
                  size="small"
                  startIcon={<RefreshRounded />}
                  onClick={reloadChannelConnectivity}
                  disabled={channelLoading}
                  sx={{ borderRadius: "var(--radius-control)" }}
                >
                  {t("device.channelRefresh")}
                </Button>
              }
            >
              <ChannelConnectivityPanel
                channels={channelList}
                loading={channelLoading}
                error={channelError}
                onRetry={reloadChannelConnectivity}
                channelLabel={(id) => t(`device.${channelNameKey[id] ?? id}`)}
                t={t}
              />
            </SettingsSection>
          </Box>

          <Box sx={{ gridColumn: { xs: "span 1", lg: "span 1" }, minWidth: 0 }}>
            <SettingsSection
              icon={<InfoOutlined sx={{ fontSize: "var(--icon-size-md)" }} />}
              label={t("device.sectionDeviceInfo")}
            >
              <Box sx={{ display: "flex", flexDirection: "column", gap: 1.75 }}>
                <SectionLoadProgress
                  loading={systemInfoLoading}
                  idleHint={!systemInfo ? t("device.deviceInfoLoading") : undefined}
                />
                {systemInfoError && !systemInfo && !systemInfoLoading && (
                  <InlineAlert
                    message={`${t("device.deviceInfoLoadFail")}: ${systemInfoError}`}
                    onRetry={reloadSystemInfo}
                  />
                )}
                {systemInfo && (
                  <Box
                    sx={{
                      display: "flex",
                      flexDirection: "column",
                      gap: 1.5,
                      opacity: systemInfoLoading ? 0.72 : 1,
                      transition: "opacity var(--transition-duration) var(--ease-emphasized)",
                      pointerEvents: systemInfoLoading ? "none" : "auto",
                    }}
                  >
                    <Row
                      label={t("device.deviceInfoProduct")}
                      value={systemInfo.product_name}
                      breakWords
                    />
                    {systemInfo.hardware_model && (
                      <Row
                        label={t("device.deviceInfoHardwareModel")}
                        value={systemInfo.hardware_model}
                        breakWords
                      />
                    )}
                    {systemInfo.board_id && (
                      <Row
                        label={t("device.deviceInfoBoardId")}
                        value={systemInfo.board_id}
                        breakWords
                      />
                    )}
                    <Row
                      label={t("device.deviceInfoLanIp")}
                      value={systemInfo.lan_ip?.trim() ? systemInfo.lan_ip : "—"}
                      valueNoWrap
                    />
                    <Row label={t("device.deviceInfoFirmware")} value={systemInfo.firmware_version} />
                    <Row label={t("device.deviceInfoStatus")} value={systemInfo.system_status} />
                    {systemInfo.locale != null && (
                      <Row label={t("device.deviceInfoLocale")} value={systemInfo.locale} />
                    )}
                    {systemInfo.ota_available != null && (
                      <Row
                        label={t("device.deviceInfoOtaAvailable")}
                        value={systemInfo.ota_available ? t("common.yes") : t("common.no")}
                      />
                    )}
                    {systemInfo.current_time && (
                      <Row
                        label={t("device.deviceInfoCurrentTime")}
                        value={systemInfo.current_time}
                        valueNoWrap
                      />
                    )}
                  </Box>
                )}
              </Box>
            </SettingsSection>
          </Box>

          <Box sx={{ gridColumn: { xs: "span 1", lg: "span 2" }, minWidth: 0 }}>
            <SettingsSection
              icon={<BeetleIcon sx={{ width: "var(--icon-size-md)", height: "var(--icon-size-md)" }} />}
              label={t("device.sectionSystemStatus")}
              accessory={
                <Button
                  variant="outlined"
                  size="small"
                  startIcon={<RefreshRounded />}
                  onClick={reloadHealth}
                  disabled={healthLoading}
                  sx={{ borderRadius: "var(--radius-control)" }}
                >
                  {t("device.channelRefresh")}
                </Button>
              }
            >
              <Box sx={{ display: "flex", flexDirection: "column", gap: 1.75 }}>
                <SectionLoadProgress
                  loading={healthLoading}
                  idleHint={!healthData ? t("device.systemStatusLoading") : undefined}
                />
                {healthError && !healthData && !healthLoading && (
                  <InlineAlert
                    message={`${t("device.systemStatusLoadFail")}: ${healthError}`}
                    onRetry={reloadHealth}
                  />
                )}
                {healthData && (
                  <Box
                    sx={{
                      opacity: healthLoading ? 0.72 : 1,
                      transition: "opacity var(--transition-duration) var(--ease-emphasized)",
                      pointerEvents: healthLoading ? "none" : "auto",
                    }}
                  >
                    <SystemStatusPanel healthData={healthData} t={t} />
                  </Box>
                )}
              </Box>
            </SettingsSection>
          </Box>
        </Box>
      )}
    </Box>
  );
}

function Row({
  label,
  value,
  valueColor = "var(--text-primary)",
  breakWords,
  valueNoWrap,
}: {
  label: string;
  value: string;
  valueColor?: string;
  breakWords?: boolean;
  /** 时间等短串：整段不换行；过长时横向滚动。 */
  valueNoWrap?: boolean;
}) {
  return (
    <Box
      sx={{
        display: "flex",
        flexWrap: valueNoWrap ? "nowrap" : "wrap",
        gap: 0.5,
        alignItems: "baseline",
        minWidth: 0,
      }}
    >
      <Typography
        variant="body2"
        sx={{ color: "var(--muted)", minWidth: 100, flexShrink: 0 }}
      >
        {label}:
      </Typography>
      <Typography
        variant="body2"
        sx={{
          fontFamily: "var(--font-mono)",
          color: valueColor,
          wordBreak: breakWords ? "break-word" : undefined,
          whiteSpace: valueNoWrap ? "nowrap" : undefined,
          minWidth: 0,
          ...(valueNoWrap
            ? { flex: 1, overflowX: "auto" as const }
            : {}),
        }}
      >
        {value}
      </Typography>
    </Box>
  );
}
