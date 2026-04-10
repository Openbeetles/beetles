import { useEffect, useMemo, useRef, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import type { SxProps, Theme } from "@mui/material/styles";
import RouterRounded from "@mui/icons-material/RouterRounded";
import ChatRounded from "@mui/icons-material/ChatRounded";
import RefreshRounded from "@mui/icons-material/RefreshRounded";
import { InlineAlert, SaveFeedback } from "../components/form";
import { ChannelConnectivityPanel } from "../components/ChannelConnectivityPanel";
import { BeetleIcon } from "../components/BeetleIcon";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { useDevice } from "../hooks/useDevice";
import {
  type SystemInfoData,
  type ChannelConnectivityItem,
  type HealthData,
  type MetricsSnapshotData,
  type ResourceSnapshotData,
} from "../api/endpoints/system";
import { fetchSystemInfoCoalesced } from "../session/systemInfoCoordinator";
import { SystemStatusPanel, LedIndicator, wifiStaLabel, yesNo, pressureColor } from "../components/SystemStatusPanel";
import { SectionLoadProgress } from "../components/SectionLoadProgress";
import { useUnsaved } from "../hooks/useUnsaved";
import { runDeferredLoading } from "../util/deferredLoading";

const DEFAULT_DEVICE_BASE_URL = "http://192.168.4.1";

function normalizeDeviceUrl(u: string): string {
  return u.trim().replace(/\/$/, "") || DEFAULT_DEVICE_BASE_URL;
}

export function DashboardCard({
  icon,
  title,
  action,
  children,
  sx,
}: {
  icon?: React.ReactNode;
  title: string;
  action?: React.ReactNode;
  children: React.ReactNode;
  sx?: SxProps<Theme>;
}) {
  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        bgcolor: "var(--card)",
        borderRadius: "var(--radius-card)",
        border: "1px solid color-mix(in srgb, var(--border) 28%, transparent)",
        boxShadow: "var(--shadow-subtle)",
        overflow: "hidden",
        height: "100%",
        transition: "transform 0.2s ease, box-shadow 0.2s ease",
        "&:hover": {
          boxShadow: "0 8px 24px color-mix(in srgb, var(--foreground) 4%, transparent)",
        },
        ...sx,
      }}
    >
      <Box
        sx={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          px: 2.5,
          py: 2,
          borderBottom: "1px solid color-mix(in srgb, var(--border) 12%, transparent)",
          bgcolor: "color-mix(in srgb, var(--foreground) 1%, transparent)",
        }}
      >
        <Box sx={{ display: "flex", alignItems: "center", gap: 1.5 }}>
          {icon && (
            <Box
              sx={{
                width: 32,
                height: 32,
                borderRadius: "var(--radius-control)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                color: "var(--primary)",
                bgcolor: "color-mix(in srgb, var(--primary) 8%, transparent)",
              }}
            >
              {icon}
            </Box>
          )}
          <Typography
            variant="subtitle2"
            sx={{
              fontWeight: 600,
              letterSpacing: "0.05em",
              color: "var(--foreground)",
              textTransform: "uppercase",
              fontSize: "0.75rem",
            }}
          >
            {title}
          </Typography>
        </Box>
        {action}
      </Box>
      <Box sx={{ p: 2.5, flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
        {children}
      </Box>
    </Box>
  );
}

export function StatRow({ label, value }: { label: string; value: string }) {
  return (
    <Box sx={{ display: "flex", justifyContent: "space-between", alignItems: "center", py: 0.5 }}>
      <Typography variant="body2" sx={{ color: "var(--muted)" }}>{label}</Typography>
      <Typography variant="body2" sx={{ fontFamily: "var(--font-mono)", fontWeight: 500 }}>{value}</Typography>
    </Box>
  );
}

export function DevicePage() {
  const { t } = useTranslation();
  const { setDirty } = useUnsaved();
  const { baseUrl, pairingCode, setBaseUrl, setPairingCode } = useDevice();
  const [urlInput, setUrlInput] = useState(baseUrl || DEFAULT_DEVICE_BASE_URL);
  const [codeInput, setCodeInput] = useState(pairingCode);
  const { api, deviceConnected } = useDeviceApi();
  const [probeStatus, setProbeStatus] = useState<"idle" | "checking" | "ok" | "fail">("idle");
  const [probeError, setProbeError] = useState("");
  const [systemInfo, setSystemInfo] = useState<SystemInfoData | null>(null);
  const [channelList, setChannelList] = useState<ChannelConnectivityItem[]>([]);
  const [channelLoading, setChannelLoading] = useState(false);
  const [channelError, setChannelError] = useState("");
  const [healthData, setHealthData] = useState<HealthData | null>(null);
  const [resourceData, setResourceData] = useState<ResourceSnapshotData | null>(null);
  const [metricsData, setMetricsData] = useState<MetricsSnapshotData | null>(null);
  const [healthLoading, setHealthLoading] = useState(false);
  const [healthError, setHealthError] = useState("");
  const [saveStatus, setSaveStatus] = useState<"idle" | "ok">("idle");

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
    const code = (pairingCode ?? "").trim();
    return runDeferredLoading({
      run: () =>
        fetchSystemInfoCoalesced(baseUrl.trim(), code, () => api.system.info(), {
          force: false,
        }),
      onSuccess: (res) => {
        if (res.ok && res.data) {
          setSystemInfo(res.data);
        }
      },
    });
  }, [api.system, deviceConnected, baseUrl, pairingCode]);

  const reloadHealth = useCallback(() => {
    setHealthError("");
    if (!deviceConnected || !baseUrl?.trim()) return;
    setHealthLoading(true);
    Promise.all([
      api.system.health(),
      api.system.resource(),
      api.system.metrics(),
    ])
      .then(([healthRes, resourceRes, metricsRes]) => {
        setHealthLoading(false);
        if (healthRes.ok && healthRes.data && resourceRes.ok && resourceRes.data && metricsRes.ok && metricsRes.data) {
          setHealthData(healthRes.data);
          setResourceData(resourceRes.data);
          setMetricsData(metricsRes.data);
        } else {
          setHealthError(healthRes.error ?? resourceRes.error ?? metricsRes.error ?? "");
        }
      })
      .catch(() => {
        setHealthLoading(false);
        setHealthError("config.errorNetwork");
      });
  }, [api.system, deviceConnected, baseUrl]);

  useEffect(() => {
    if (!deviceConnected || !baseUrl?.trim()) return;
    let mounted = true;
    setHealthLoading(true);
    Promise.all([api.system.health(), api.system.resource(), api.system.metrics()])
      .then(([healthRes, resourceRes, metricsRes]) => {
        if (!mounted) return;
        setHealthLoading(false);
        if (healthRes.ok && healthRes.data && resourceRes.ok && resourceRes.data && metricsRes.ok && metricsRes.data) {
          setHealthData(healthRes.data);
          setResourceData(resourceRes.data);
          setMetricsData(metricsRes.data);
        } else {
          setHealthError(healthRes.error ?? resourceRes.error ?? metricsRes.error ?? "");
        }
      })
      .catch(() => {
        if (!mounted) return;
        setHealthLoading(false);
        setHealthError("config.errorNetwork");
      });
    return () => { mounted = false; };
  }, [api.system, deviceConnected, baseUrl]);

  const reloadChannelConnectivity = useCallback(() => {
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
  }, [api.system, deviceConnected, baseUrl]);

  useEffect(() => {
    if (!deviceConnected || !baseUrl?.trim()) return;
    let mounted = true;
    setChannelLoading(true);
    api.system
      .channelConnectivity()
      .then((res) => {
        if (!mounted) return;
        setChannelLoading(false);
        if (res.ok && res.data?.channels) setChannelList(res.data.channels);
        else setChannelError(res.error ?? "channel connectivity unavailable");
      })
      .catch(() => {
        if (!mounted) return;
        setChannelLoading(false);
        setChannelError("config.errorNetwork");
      });
    return () => { mounted = false; };
  }, [api.system, deviceConnected, baseUrl]);

  const channelNameKey: Record<string, string> = {
    telegram: "channelTelegram",
    feishu: "channelFeishu",
    dingtalk: "channelDingtalk",
    wecom: "channelWecom",
    qq_channel: "channelQqChannel",
    webhook: "channelWebhook",
  };

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 3 }}>
      <InlineAlert
        message={probeStatus === "fail" ? `${t("device.probeFail")}: ${probeError}` : null}
        onRetry={handleProbe}
      />
      
      {/* Global Status Bar (Top Bar) */}
      <Box
        sx={{
          bgcolor: "color-mix(in srgb, var(--foreground) 2%, transparent)",
          borderRadius: "var(--radius-card)",
          p: 1.5,
          px: 2.5,
          display: "flex",
          flexWrap: "wrap",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 2,
          width: "100%",
          boxShadow: "var(--shadow-subtle)",
          border: "1px solid color-mix(in srgb, var(--border) 20%, transparent)",
        }}
      >
        {/* Left: Connection Controls */}
        <Box sx={{ display: "flex", alignItems: "center", gap: 1.5, flexWrap: "wrap" }}>
          <RouterRounded sx={{ color: "var(--primary)", opacity: 0.8 }} />
          <TextField
            placeholder={t("device.baseUrlPlaceholder")}
            value={urlInput}
            onChange={(e) => {
              setUrlInput(e.target.value);
              setSaveStatus("idle");
            }}
            size="small"
            variant="standard"
            slotProps={{ htmlInput: { style: { fontFamily: "var(--font-mono)", fontSize: "0.875rem" } }, input: { disableUnderline: true } }}
            sx={{ minWidth: 200 }}
          />
          <Box sx={{ width: "1px", height: 20, bgcolor: "var(--border-subtle)" }} />
          <TextField
            placeholder={t("device.pairingCodePlaceholder")}
            value={codeInput}
            onChange={(e) => {
              setCodeInput(e.target.value);
              setSaveStatus("idle");
            }}
            size="small"
            variant="standard"
            slotProps={{
              htmlInput: { maxLength: 6, style: { fontFamily: "var(--font-mono)", fontSize: "0.875rem" } },
              input: { disableUnderline: true }
            }}
            sx={{ width: 100 }}
          />
          <Box sx={{ display: "flex", gap: 1, ml: 1 }}>
            <Button
              variant="contained"
              size="small"
              onClick={handleSave}
              sx={{ borderRadius: "9999px", px: 2, minWidth: 0, py: 0.5 }}
            >
              {t("device.save")}
            </Button>
            <Button
              variant="outlined"
              size="small"
              onClick={handleProbe}
              disabled={probeStatus === "checking"}
              sx={{ borderRadius: "9999px", px: 2, minWidth: 0, py: 0.5 }}
            >
              {probeStatus === "checking" ? t("device.probing") : t("device.probe")}
            </Button>
          </Box>
          {probeStatus === "ok" && (
            <Typography variant="caption" sx={{ color: "var(--semantic-success)", ml: 1 }}>
              {t("device.probeOk")}
            </Typography>
          )}
          {saveStatus === "ok" && (
            <Box sx={{ ml: 1 }}>
              <SaveFeedback status="ok" message={t("common.saveOk")} autoDismissMs={3000} onDismiss={() => setSaveStatus("idle")} />
            </Box>
          )}
        </Box>

        {/* Right: Global LEDs */}
        {deviceConnected && healthData && resourceData && (
          <Box sx={{ display: "flex", alignItems: "center", gap: 1.5, flexWrap: "wrap" }}>
            <LedIndicator 
              active={true} 
              color={healthData.wifi === "connected" ? "var(--semantic-success)" : "var(--semantic-warning)"} 
              label={`${t("device.systemStatusWifiSta")}: ${wifiStaLabel(healthData.wifi, t)}`} 
            />
            <LedIndicator 
              active={true} 
              color={healthData.display?.available === true ? "var(--semantic-success)" : "var(--muted)"} 
              label={`${t("device.systemStatusDisplayAvailable")}: ${yesNo(healthData.display?.available, t)}`} 
            />
            <LedIndicator 
              active={true} 
              color={pressureColor(resourceData.pressure)} 
              label={`${t("device.systemStatusPressure")}: ${resourceData.pressure ?? t("common.na")}`} 
            />
          </Box>
        )}
      </Box>

      {deviceConnected && (
        <Box sx={{ display: "flex", flexDirection: "column", gap: 3 }}>
          <Box sx={{ display: "flex", justifyContent: "flex-end", gap: 2 }}>
            <Button variant="text" size="small" startIcon={<RefreshRounded />} onClick={() => { reloadHealth(); reloadChannelConnectivity(); }} disabled={healthLoading || channelLoading}>
              {t("device.channelRefresh")}
            </Button>
          </Box>

          <SectionLoadProgress loading={healthLoading} idleHint={!healthData ? t("device.systemStatusLoading") : undefined} />
          {healthError && !healthData && !healthLoading && (
            <InlineAlert message={`${t("device.systemStatusLoadFail")}: ${healthError}`} onRetry={reloadHealth} />
          )}

          {healthData && resourceData && metricsData && (
            <Box
              sx={{
                display: "grid",
                gridTemplateColumns: { xs: "repeat(4, 1fr)", sm: "repeat(8, 1fr)", lg: "repeat(12, 1fr)" },
                gridAutoRows: "minmax(120px, auto)",
                gridAutoFlow: "row dense",
                gap: 2.5,
                opacity: healthLoading ? 0.72 : 1,
                transition: "opacity var(--transition-duration) var(--ease-emphasized)",
                pointerEvents: healthLoading ? "none" : "auto",
              }}
            >
              {/* Hero Section: Device Identity (4 cols) - Compact & Centered */}
              <Box sx={{ gridColumn: { xs: "span 4", sm: "span 4", lg: "span 4" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
                <DashboardCard title="Beetle OS" icon={<BeetleIcon />}>
                  <Box sx={{ display: "flex", flexDirection: "column", height: "100%", alignItems: "center", justifyContent: "center", textAlign: "center", py: 1 }}>
                    <Box sx={{ width: 56, height: 56, borderRadius: "50%", bgcolor: "color-mix(in srgb, var(--primary) 10%, transparent)", display: "flex", alignItems: "center", justifyContent: "center", mb: 2, color: "var(--primary)" }}>
                      <BeetleIcon sx={{ fontSize: 28 }} />
                    </Box>
                    <Typography variant="h4" sx={{ fontWeight: 700, letterSpacing: "-0.02em", mb: 0.5, color: "var(--foreground)" }}>
                      {systemInfo?.product_name || "Beetle Device"}
                    </Typography>
                    <Typography variant="body2" sx={{ color: "var(--muted)", mb: 4, fontFamily: "var(--font-mono)" }}>
                      {systemInfo?.board_id || "Unknown Board"}
                    </Typography>
                    
                    <Box sx={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 1.5, width: "100%", mt: "auto" }}>
                      <Box sx={{ p: 1.5, bgcolor: "color-mix(in srgb, var(--foreground) 3%, transparent)", borderRadius: "var(--radius-chip)", border: "1px solid color-mix(in srgb, var(--border) 40%, transparent)" }}>
                        <Typography variant="caption" sx={{ color: "var(--muted)", display: "block", textTransform: "uppercase", letterSpacing: "0.05em", mb: 0.5 }}>{t("device.deviceInfoLanIp")}</Typography>
                        <Typography variant="body2" sx={{ fontFamily: "var(--font-mono)", fontWeight: 600, color: "var(--primary)" }}>{systemInfo?.lan_ip || "—"}</Typography>
                      </Box>
                      <Box sx={{ p: 1.5, bgcolor: "color-mix(in srgb, var(--foreground) 3%, transparent)", borderRadius: "var(--radius-chip)", border: "1px solid color-mix(in srgb, var(--border) 40%, transparent)" }}>
                        <Typography variant="caption" sx={{ color: "var(--muted)", display: "block", textTransform: "uppercase", letterSpacing: "0.05em", mb: 0.5 }}>{t("device.deviceInfoFirmware")}</Typography>
                        <Typography variant="body2" sx={{ fontFamily: "var(--font-mono)", fontWeight: 600 }}>{systemInfo?.firmware_version || "—"}</Typography>
                      </Box>
                    </Box>
                  </Box>
                </DashboardCard>
              </Box>

              {/* Channel Connectivity (8 cols) - Expanded to give the table room to breathe */}
              <Box sx={{ gridColumn: { xs: "span 4", sm: "span 8", lg: "span 8" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
                <DashboardCard title={t("device.sectionChannelConnectivity")} icon={<ChatRounded />}>
                  <ChannelConnectivityPanel
                    channels={channelList}
                    loading={channelLoading}
                    error={channelError}
                    onRetry={reloadChannelConnectivity}
                    channelLabel={(id) => t(`device.${channelNameKey[id] ?? id}`)}
                    t={t}
                  />
                </DashboardCard>
              </Box>

              <SystemStatusPanel
                healthData={healthData}
                resourceData={resourceData}
                metricsData={metricsData}
                t={t}
              />
            </Box>
          )}
        </Box>
      )}
    </Box>
  );
}
