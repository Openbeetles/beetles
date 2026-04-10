import { useEffect, useMemo, useRef, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import type { SxProps, Theme } from "@mui/material/styles";
import DeveloperBoardOutlined from "@mui/icons-material/DeveloperBoardOutlined";
import ChatRounded from "@mui/icons-material/ChatRounded";
import RefreshRounded from "@mui/icons-material/RefreshRounded";
import LinkRounded from "@mui/icons-material/LinkRounded";
import { InlineAlert } from "../components/form";
import { ChannelConnectivityPanel } from "../components/ChannelConnectivityPanel";
import { PcbDecorOverlay } from "../components/PcbDecorOverlay";
import { BeetleIcon } from "../components/BeetleIcon";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { useDevice } from "../hooks/useDevice";
import { useRevealedPassword } from "../hooks/useRevealedPassword";
import { useToast } from "../hooks/useToast";
import { useDeviceRuntimeKind } from "../store/deviceStatusStore";
import {
  type SystemInfoData,
  type ChannelConnectivityItem,
  type HealthData,
  type MetricsSnapshotData,
  type ResourceSnapshotData,
} from "../api/endpoints/system";
import { fetchSystemInfoCoalesced } from "../session/systemInfoCoordinator";
import {
  pressureColor,
  wifiStaLabel,
  yesNo,
} from "../components/deviceStatusBarHelpers";
import { SystemStatusPanel } from "../components/SystemStatusPanel";
import { SectionLoadProgress } from "../components/SectionLoadProgress";
import { useUnsaved } from "../hooks/useUnsaved";
import { runDeferredLoading } from "../util/deferredLoading";
import {
  audioProfileLabelKey,
  buildDeviceOperationalStatusKey,
  buildDeviceSummaryFields,
  pressureLabelKey,
} from "./deviceHomeViewModel";
import {
  DASHBOARD_CARD_BODY_SX,
  DASHBOARD_CARD_HEADER_ROW_SX,
  DASHBOARD_CARD_SURFACE_SX,
  DASHBOARD_HOME_GRID_GAP,
  DASHBOARD_INSET_WELL_BG,
  UI_LABEL_SECONDARY_SX,
} from "../theme/panelStyles";

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
        height: "100%",
        ...DASHBOARD_CARD_SURFACE_SX,
        ...sx,
      }}
    >
      <Box sx={DASHBOARD_CARD_HEADER_ROW_SX}>
        <Box sx={{ display: "flex", alignItems: "center", gap: 1.5, minWidth: 0 }}>
          {icon && (
            <Box
              sx={{
                width: "var(--icon-size-lg)",
                height: "var(--icon-size-lg)",
                borderRadius: "var(--radius-control)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                color: "var(--primary)",
                bgcolor: "color-mix(in srgb, var(--primary) 8%, transparent)",
                boxShadow:
                  "inset 0 0 0 1px color-mix(in srgb, var(--primary) 15%, transparent)",
                flexShrink: 0,
              }}
            >
              {icon}
            </Box>
          )}
          <Typography
            variant="subtitle2"
            sx={{
              fontWeight: 600,
              letterSpacing: "var(--letter-spacing-label)",
              color: "var(--foreground)",
              textTransform: "none",
              fontSize: "0.8125rem",
              lineHeight: 1.35,
            }}
          >
            {title}
          </Typography>
        </Box>
        {action}
      </Box>
      <Box sx={DASHBOARD_CARD_BODY_SX}>
        {children}
      </Box>
    </Box>
  );
}

export function StatRow({ label, value }: { label: string; value: string }) {
  return (
    <Box
      sx={{
        display: "flex",
        justifyContent: "space-between",
        alignItems: "center",
        gap: 1.5,
        py: 1.1,
      }}
    >
      <Typography
        variant="body2"
        sx={{
          ...UI_LABEL_SECONDARY_SX,
          fontSize: "0.8rem",
          flex: "1 1 auto",
          minWidth: 0,
        }}
      >
        {label}
      </Typography>
      <Typography
        variant="body2"
        sx={{
          fontFamily: "var(--font-mono)",
          fontWeight: 500,
          fontSize: "var(--font-size-data-value)",
          color: "var(--foreground)",
          textAlign: "right",
          flexShrink: 0,
        }}
      >
        {value}
      </Typography>
    </Box>
  );
}

export function DevicePage() {
  const { t } = useTranslation();
  const { setDirty } = useUnsaved();
  const { baseUrl, pairingCode, setBaseUrl, setPairingCode } = useDevice();
  const runtimeKind = useDeviceRuntimeKind();
  const [urlInput, setUrlInput] = useState(baseUrl || DEFAULT_DEVICE_BASE_URL);
  const [codeInput, setCodeInput] = useState(pairingCode);
  const pairingCodeReveal = useRevealedPassword();
  const { api, deviceConnected } = useDeviceApi();
  const { showToast } = useToast();
  const [probeStatus, setProbeStatus] = useState<
    "idle" | "checking" | "ok" | "fail"
  >("idle");
  const [systemInfo, setSystemInfo] = useState<SystemInfoData | null>(null);
  const [channelList, setChannelList] = useState<ChannelConnectivityItem[]>([]);
  const [channelLoading, setChannelLoading] = useState(false);
  const [channelError, setChannelError] = useState("");
  const [healthData, setHealthData] = useState<HealthData | null>(null);
  const [resourceData, setResourceData] = useState<ResourceSnapshotData | null>(
    null,
  );
  const [metricsData, setMetricsData] = useState<MetricsSnapshotData | null>(
    null,
  );
  const [healthLoading, setHealthLoading] = useState(false);
  const [healthError, setHealthError] = useState("");

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

  const runtimeStatusKey = useMemo(
    () => buildDeviceOperationalStatusKey(healthData, resourceData),
    [healthData, resourceData],
  );
  const deviceSummaryFields = useMemo(
    () => buildDeviceSummaryFields(systemInfo, healthData, runtimeStatusKey),
    [systemInfo, healthData, runtimeStatusKey],
  );

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
    showToast(t("common.saveOk"), { variant: "success" });
  };

  const handleProbe = async () => {
    const url = urlInput.trim().replace(/\/$/, "") || DEFAULT_DEVICE_BASE_URL;
    setProbeStatus("checking");
    const res = await api.device.probe(url);
    if (res.ok) {
      setProbeStatus("ok");
      showToast(t("device.probeOk"), { variant: "success" });
    } else {
      setProbeStatus("fail");
      showToast(`${t("device.probeFail")}: ${res.error || t("common.error")}`, { variant: "error" });
    }
  };

  useEffect(() => {
    if (!deviceConnected || !baseUrl?.trim()) return;
    const code = (pairingCode ?? "").trim();
    return runDeferredLoading({
      run: () =>
        fetchSystemInfoCoalesced(
          baseUrl.trim(),
          code,
          () => api.system.info(),
          {
            force: false,
          },
        ),
      onStart: () => {},
      onSuccess: (res) => {
        if (res.ok && res.data) {
          setSystemInfo(res.data);
        }
      },
      onError: () => {},
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
        if (
          healthRes.ok &&
          healthRes.data &&
          resourceRes.ok &&
          resourceRes.data &&
          metricsRes.ok &&
          metricsRes.data
        ) {
          setHealthData(healthRes.data);
          setResourceData(resourceRes.data);
          setMetricsData(metricsRes.data);
        } else {
          const err = healthRes.error ?? resourceRes.error ?? metricsRes.error ?? "";
          setHealthError(err);
          showToast(`${t("device.systemStatusLoadFail")}: ${err}`, { variant: "error" });
        }
      })
      .catch(() => {
        setHealthLoading(false);
        setHealthError("config.errorNetwork");
        showToast(t("config.errorNetwork"), { variant: "error" });
      });
  }, [api.system, deviceConnected, baseUrl, showToast, t]);

  useEffect(() => {
    if (!deviceConnected || !baseUrl?.trim()) return;
    let mounted = true;
    // Mount sync: enter loading before fetch (same as `reloadHealth`).
    // eslint-disable-next-line react-hooks/set-state-in-effect -- intentional loading flag for initial fetch after connect
    setHealthLoading(true);
    void Promise.all([
      api.system.health(),
      api.system.resource(),
      api.system.metrics(),
    ])
      .then(([healthRes, resourceRes, metricsRes]) => {
        if (!mounted) return;
        setHealthLoading(false);
        if (
          healthRes.ok &&
          healthRes.data &&
          resourceRes.ok &&
          resourceRes.data &&
          metricsRes.ok &&
          metricsRes.data
        ) {
          setHealthData(healthRes.data);
          setResourceData(resourceRes.data);
          setMetricsData(metricsRes.data);
        } else {
          setHealthError(
            healthRes.error ?? resourceRes.error ?? metricsRes.error ?? "",
          );
        }
      })
      .catch(() => {
        if (!mounted) return;
        setHealthLoading(false);
        setHealthError("config.errorNetwork");
      });
    return () => {
      mounted = false;
    };
  }, [api.system, deviceConnected, baseUrl]);

  const reloadChannelConnectivity = useCallback(() => {
    setChannelError("");
    if (!deviceConnected || !baseUrl?.trim()) return;
    setChannelLoading(true);
    api.system
      .channelConnectivity()
      .then((res) => {
        setChannelLoading(false);
        if (res.ok && res.data?.channels) {
          setChannelList(res.data.channels);
        } else {
          const err = res.error ?? "channel connectivity unavailable";
          setChannelError(err);
          showToast(`${t("device.channelConnectivityLoadFailedTitle")}: ${err}`, { variant: "error" });
        }
      })
      .catch(() => {
        setChannelLoading(false);
        setChannelError("config.errorNetwork");
        showToast(t("config.errorNetwork"), { variant: "error" });
      });
  }, [api.system, deviceConnected, baseUrl, showToast, t]);

  useEffect(() => {
    if (!deviceConnected || !baseUrl?.trim()) return;
    let mounted = true;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- intentional loading flag for initial fetch after connect
    setChannelLoading(true);
    void api.system
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
    return () => {
      mounted = false;
    };
  }, [api.system, deviceConnected, baseUrl]);

  const channelNameKey: Record<string, string> = {
    telegram: "channelTelegram",
    feishu: "channelFeishu",
    dingtalk: "channelDingtalk",
    wecom: "channelWecom",
    qq_channel: "channelQqChannel",
    webhook: "channelWebhook",
  };

  const pressureLabel =
    pressureLabelKey(resourceData?.pressure) != null
      ? t(pressureLabelKey(resourceData?.pressure) as string)
      : t("common.na");
  const audioProfileLabel =
    audioProfileLabelKey(healthData?.audio?.duplex_profile) != null
      ? t(audioProfileLabelKey(healthData?.audio?.duplex_profile) as string)
      : t("common.na");
  const audioCapabilityRows = [
    {
      label: t("device.audioCapabilityMicrophoneInput"),
      value: yesNo(healthData?.audio?.duplex_capabilities?.microphone_input, t),
      active: healthData?.audio?.duplex_capabilities?.microphone_input === true,
    },
    {
      label: t("device.audioCapabilitySpeakerOutput"),
      value: yesNo(healthData?.audio?.duplex_capabilities?.speaker_output, t),
      active: healthData?.audio?.duplex_capabilities?.speaker_output === true,
    },
  ];

  const renderSummaryFieldValue = (
    value: string | boolean,
    valueKind: "text" | "boolean" | "audio_profile" | "status_key",
  ): string => {
    if (valueKind === "boolean") return yesNo(value as boolean, t);
    if (valueKind === "audio_profile") {
      const key = audioProfileLabelKey(String(value));
      return key ? t(key) : String(value);
    }
    if (valueKind === "status_key") return t(String(value));
    return String(value);
  };

  const renderHeroCard = () => (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        position: "relative",
        ...DASHBOARD_CARD_SURFACE_SX,
        p: { xs: 3, md: 4 },
      }}
    >
      <PcbDecorOverlay tone="embed" />
      <Box sx={{ position: "relative", zIndex: 1, display: "flex", justifyContent: "space-between", alignItems: "flex-start", flexWrap: "wrap", gap: 2 }}>
        <Box sx={{ display: "flex", gap: 3, alignItems: "center" }}>
          <Box
            sx={{
              width: 72,
              height: 72,
              borderRadius: 4,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              bgcolor: "color-mix(in srgb, var(--primary) 12%, transparent)",
              color: "var(--primary)",
              boxShadow: "0 8px 16px color-mix(in srgb, var(--primary) 10%, transparent)",
              flexShrink: 0,
            }}
          >
            <BeetleIcon sx={{ width: 40, height: 40 }} />
          </Box>
          <Box sx={{ minWidth: 0 }}>
            <Typography
              variant="h3"
              sx={{
                fontFamily: "var(--font-display)",
                fontWeight: 800,
                color: "var(--foreground)",
                letterSpacing: "-0.02em",
                lineHeight: 1.1,
                mb: 1.5,
                whiteSpace: "nowrap",
                overflow: "hidden",
                textOverflow: "ellipsis",
              }}
            >
              {(systemInfo?.product_name || "beetle").toUpperCase()}
              <Box component="span" sx={{ color: "var(--primary)", ml: 1.5, fontWeight: 700 }}>
                OS
              </Box>
            </Typography>
            <Box sx={{ display: "flex", gap: 1.5, alignItems: "center", flexWrap: "wrap" }}>
              <Box
                sx={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: 0.75,
                  px: 1.25,
                  py: 0.5,
                  borderRadius: "var(--radius-full)",
                  bgcolor: "color-mix(in srgb, var(--semantic-success) 12%, transparent)",
                }}
              >
                <Box sx={{ width: 8, height: 8, borderRadius: "50%", bgcolor: "var(--semantic-success)", boxShadow: "0 0 8px var(--semantic-success)" }} />
                <Typography variant="caption" sx={{ color: "var(--semantic-success)", fontWeight: 700, letterSpacing: "0.05em", lineHeight: 1 }}>
                  {t("device.sysStatusOnline").toUpperCase()}
                </Typography>
              </Box>
              {systemInfo?.lan_ip && (
                <Typography variant="caption" sx={{ fontFamily: "var(--font-mono)", color: "var(--foreground-soft)", bgcolor: DASHBOARD_INSET_WELL_BG, px: 1.25, py: 0.5, borderRadius: "var(--radius-full)", fontWeight: 600, lineHeight: 1 }}>
                  {systemInfo.lan_ip}
                </Typography>
              )}
              {systemInfo?.firmware_version && (
                <Typography variant="caption" sx={{ fontFamily: "var(--font-mono)", color: "var(--foreground-soft)", bgcolor: DASHBOARD_INSET_WELL_BG, px: 1.25, py: 0.5, borderRadius: "var(--radius-full)", fontWeight: 600, lineHeight: 1 }}>
                  v{systemInfo.firmware_version}
                </Typography>
              )}
            </Box>
          </Box>
        </Box>
        <Button
          variant="outlined"
          size="small"
          startIcon={<RefreshRounded />}
          onClick={() => {
            reloadHealth();
            reloadChannelConnectivity();
          }}
          disabled={healthLoading || channelLoading}
          sx={{ borderRadius: "var(--radius-full)", bgcolor: "color-mix(in srgb, var(--card) 50%, transparent)", backdropFilter: "blur(10px)" }}
        >
          {t("device.channelRefresh")}
        </Button>
      </Box>

      <Box sx={{ flexGrow: 1, minHeight: { xs: 24, sm: 28 } }} />

      {/* Hardware LEDs Strip */}
      {healthData && resourceData && (
        <Box sx={{ position: "relative", zIndex: 1, display: "flex", flexWrap: "wrap", gap: 1.25, mt: { xs: 3, md: 3.5 } }}>
          {[
            { label: t("device.systemStatusWifiSta"), value: wifiStaLabel(healthData.wifi, t), color: healthData.wifi === "connected" ? "var(--semantic-success)" : "var(--semantic-warning)", active: true },
            { label: t("device.systemStatusDisplayAvailable"), value: yesNo(healthData.display?.available, t), color: healthData.display?.available ? "var(--semantic-success)" : "var(--muted)", active: healthData.display?.available },
            { label: t("device.systemStatusPressure"), value: pressureLabel, color: pressureColor(resourceData.pressure), active: true },
            { label: t("device.deviceInfoAudio"), value: audioProfileLabel, color: healthData.audio?.duplex_profile && healthData.audio.duplex_profile !== "unavailable" ? "var(--semantic-success)" : "var(--muted)", active: healthData.audio?.duplex_profile && healthData.audio.duplex_profile !== "unavailable" },
            ...audioCapabilityRows.map(r => ({ label: r.label, value: r.value, color: r.active ? "var(--semantic-success)" : "var(--muted)", active: r.active }))
          ].map((led, idx) => (
            <Box
              key={idx}
              sx={{
                display: "flex",
                alignItems: "center",
                gap: 1.15,
                px: 1.5,
                py: 1.1,
                borderRadius: "var(--radius-chip)",
                bgcolor: DASHBOARD_INSET_WELL_BG,
                flex: "1 1 148px",
                minWidth: 132,
              }}
            >
              <Box sx={{ width: 8, height: 8, borderRadius: "50%", bgcolor: led.color, boxShadow: led.active ? `0 0 10px ${led.color}` : "none", flexShrink: 0 }} />
              <Box sx={{ minWidth: 0 }}>
                <Typography variant="caption" sx={{ display: "block", fontSize: "0.65rem", color: "var(--muted)", textTransform: "uppercase", letterSpacing: "0.06em", lineHeight: 1, mb: 0.45, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                  {led.label}
                </Typography>
                <Typography variant="body2" sx={{ fontFamily: "var(--font-mono)", fontSize: "var(--font-size-data-value)", fontWeight: 600, color: "var(--foreground)", lineHeight: 1.15, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                  {led.value}
                </Typography>
              </Box>
            </Box>
          ))}
        </Box>
      )}
    </Box>
  );

  const renderConnectionCard = () => (
    <DashboardCard title={t("device.sectionConnection")} icon={<LinkRounded />} sx={{ height: "100%" }}>
      <Box sx={{ display: "flex", flexDirection: "column", gap: 2.5, height: "100%", justifyContent: "center" }}>
        <TextField
          label={t("device.baseUrlLabel")}
          placeholder={t("device.baseUrlPlaceholder")}
          value={urlInput}
          onChange={(e) => setUrlInput(e.target.value)}
          size="small"
          variant="outlined"
          fullWidth
          slotProps={{
            htmlInput: { style: { fontFamily: "var(--font-mono)", fontSize: "0.875rem" } },
          }}
          sx={{ '& .MuiOutlinedInput-root': { borderRadius: 2, bgcolor: "color-mix(in srgb, var(--border) 10%, transparent)" } }}
        />
        <TextField
          label={t("device.pairingCodeLabel")}
          placeholder={t("device.pairingCodePlaceholder")}
          value={codeInput}
          type={pairingCodeReveal.type}
          onChange={(e) => setCodeInput(e.target.value)}
          size="small"
          variant="outlined"
          fullWidth
          slotProps={{
            htmlInput: {
              maxLength: 6,
              style: { fontFamily: "var(--font-mono)", fontSize: "0.875rem", letterSpacing: "0.2em" },
              ...pairingCodeReveal.inputProps,
            },
          }}
          sx={{ '& .MuiOutlinedInput-root': { borderRadius: 2, bgcolor: "color-mix(in srgb, var(--border) 10%, transparent)" } }}
        />
        <Box sx={{ display: "flex", gap: 1.5, mt: "auto", pt: 1 }}>
          <Button variant="contained" onClick={handleSave} fullWidth sx={{ borderRadius: "var(--radius-full)", py: 1, fontWeight: 600, boxShadow: "none", "&:hover": { boxShadow: "none" } }}>
            {t("device.save")}
          </Button>
          <Button variant="outlined" onClick={handleProbe} disabled={probeStatus === "checking"} fullWidth sx={{ borderRadius: "var(--radius-full)", py: 1, fontWeight: 600 }}>
            {probeStatus === "checking" ? t("device.probing") : t("device.probe")}
          </Button>
        </Box>
      </Box>
    </DashboardCard>
  );

  const renderSetupCard = () => (
    <Box sx={{ width: "100%", maxWidth: 480, mx: "auto", mt: { xs: 4, md: 8 }, ...DASHBOARD_CARD_SURFACE_SX, p: { xs: 3, md: 5 }, position: "relative", display: "flex", flexDirection: "column", gap: 4 }}>
      <PcbDecorOverlay tone="embed" />
      <Box sx={{ position: "relative", zIndex: 1, textAlign: "center" }}>
        <Box sx={{ width: 88, height: 88, mx: "auto", mb: 3, borderRadius: 5, bgcolor: "color-mix(in srgb, var(--primary) 10%, transparent)", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--primary)", boxShadow: "0 12px 24px color-mix(in srgb, var(--primary) 10%, transparent)" }}>
          <BeetleIcon sx={{ fontSize: 48 }} />
        </Box>
        <Typography variant="h4" sx={{ fontFamily: "var(--font-display)", fontWeight: 800, letterSpacing: "-0.02em", mb: 1.5 }}>
          beetle <Box component="span" sx={{ color: "var(--primary)" }}>OS</Box>
        </Typography>
        <Typography variant="body2" sx={{ color: "var(--text-secondary)", lineHeight: 1.6, maxWidth: "80%", mx: "auto" }}>
          {t("device.pageDesc")}
        </Typography>
      </Box>

      <Box sx={{ position: "relative", zIndex: 1, display: "flex", flexDirection: "column", gap: 2.5 }}>
        <TextField
          label={t("device.baseUrlLabel")}
          placeholder={t("device.baseUrlPlaceholder")}
          value={urlInput}
          onChange={(e) => setUrlInput(e.target.value)}
          variant="outlined"
          fullWidth
          slotProps={{ htmlInput: { style: { fontFamily: "var(--font-mono)" } } }}
          sx={{ '& .MuiOutlinedInput-root': { borderRadius: 2, bgcolor: "color-mix(in srgb, var(--border) 10%, transparent)" } }}
        />
        <TextField
          label={t("device.pairingCodeLabel")}
          placeholder={t("device.pairingCodePlaceholder")}
          value={codeInput}
          type={pairingCodeReveal.type}
          onChange={(e) => setCodeInput(e.target.value)}
          variant="outlined"
          fullWidth
          slotProps={{
            htmlInput: { maxLength: 6, style: { fontFamily: "var(--font-mono)", letterSpacing: "0.2em" }, ...pairingCodeReveal.inputProps }
          }}
          sx={{ '& .MuiOutlinedInput-root': { borderRadius: 2, bgcolor: "color-mix(in srgb, var(--border) 10%, transparent)" } }}
        />
        <Box sx={{ display: "flex", gap: 1.5, mt: 1 }}>
          <Button variant="contained" onClick={handleSave} fullWidth size="large" sx={{ borderRadius: "var(--radius-full)", fontWeight: 600, boxShadow: "none", py: 1.25, "&:hover": { boxShadow: "none" } }}>
            {t("device.save")}
          </Button>
          <Button variant="outlined" onClick={handleProbe} disabled={probeStatus === "checking"} fullWidth size="large" sx={{ borderRadius: "var(--radius-full)", fontWeight: 600, py: 1.25 }}>
            {probeStatus === "checking" ? t("device.probing") : t("device.probe")}
          </Button>
        </Box>
      </Box>
    </Box>
  );

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: DASHBOARD_HOME_GRID_GAP }}>
      {!deviceConnected ? (
        renderSetupCard()
      ) : (
        <Box sx={{ display: "flex", flexDirection: "column", gap: DASHBOARD_HOME_GRID_GAP }}>
          <Box
            sx={{
              display: "grid",
              gridTemplateColumns: {
                xs: "repeat(4, 1fr)",
                sm: "repeat(8, 1fr)",
                lg: "repeat(12, 1fr)",
              },
              gridAutoRows: "minmax(120px, auto)",
              gridAutoFlow: "row dense",
              gap: DASHBOARD_HOME_GRID_GAP,
              opacity: 1,
              pointerEvents: "auto",
            }}
          >
            {/* Row 1: Hero (8) + Connection (4) */}
            <Box sx={{ gridColumn: { xs: "span 4", sm: "span 8", lg: "span 8" }, gridRow: { xs: "span 2", sm: "span 2", lg: "span 2" } }}>
              {renderHeroCard()}
            </Box>

            <Box sx={{ gridColumn: { xs: "span 4", sm: "span 4", lg: "span 4" }, gridRow: { xs: "span 2", sm: "span 2", lg: "span 2" } }}>
              {renderConnectionCard()}
            </Box>

            {/* If loading or error, show them in the remaining space */}
            {(!healthData || !resourceData || !metricsData) ? (
              <Box sx={{ gridColumn: { xs: "span 4", sm: "span 8", lg: "span 12" }, gridRow: { xs: "span 1", lg: "span 1" }, display: "flex", flexDirection: "column", gap: 2, justifyContent: "center" }}>
                <SectionLoadProgress loading={healthLoading} idleHint={!healthData ? t("device.systemStatusLoading") : undefined} />
                {healthError && !healthData && !healthLoading && (
                  <InlineAlert message={`${t("device.systemStatusLoadFail")}: ${healthError}`} onRetry={reloadHealth} />
                )}
              </Box>
            ) : (
              <>
                {/* Row 2: Device Details (4) + Channels (8) */}
                <Box sx={{ gridColumn: { xs: "span 4", sm: "span 4", lg: "span 4" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
                  <DashboardCard title={t("device.sectionDeviceInfo")} icon={<DeveloperBoardOutlined />}>
                    <Box sx={{ display: "flex", flexDirection: "column", gap: 0 }}>
                      {deviceSummaryFields.map((field) => (
                        <StatRow key={field.id} label={t(field.labelKey)} value={renderSummaryFieldValue(field.value, field.valueKind)} />
                      ))}
                    </Box>
                  </DashboardCard>
                </Box>

                <Box sx={{ gridColumn: { xs: "span 4", sm: "span 8", lg: "span 8" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
                  <DashboardCard title={t("device.sectionChannelConnectivity")} icon={<ChatRounded />}>
                    <ChannelConnectivityPanel channels={channelList} loading={channelLoading} error={channelError} onRetry={reloadChannelConnectivity} channelLabel={(id) => t(`device.${channelNameKey[id] ?? id}`)} t={t} />
                  </DashboardCard>
                </Box>

                <SystemStatusPanel healthData={healthData} resourceData={resourceData} metricsData={metricsData} runtimeKind={runtimeKind} t={t} />
              </>
            )}
          </Box>
        </Box>
      )}
    </Box>
  );
}
