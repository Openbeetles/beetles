import { useEffect, useMemo, useRef, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import type { SxProps, Theme } from "@mui/material/styles";
import MemoryRounded from "@mui/icons-material/MemoryRounded";
import ChatRounded from "@mui/icons-material/ChatRounded";
import RefreshRounded from "@mui/icons-material/RefreshRounded";
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
  DASHBOARD_CARD_HEADER_ROW_SX,
  DASHBOARD_CARD_SURFACE_SX,
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
      <Box
        sx={{
          p: 2.5,
          flex: 1,
          display: "flex",
          flexDirection: "column",
          minHeight: 0,
        }}
      >
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
        py: 1,
        borderBottom: "1px solid color-mix(in srgb, var(--border) 20%, transparent)",
        "&:last-child": { borderBottom: "none" },
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

  const renderGatewayCard = () => (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        position: "relative",
        ...DASHBOARD_CARD_SURFACE_SX,
      }}
    >
      <PcbDecorOverlay tone="embed" />

      <Box
        sx={{
          ...DASHBOARD_CARD_HEADER_ROW_SX,
          position: "relative",
          zIndex: 1,
          gap: 1.5,
        }}
      >
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            gap: 2,
            minWidth: 0,
            flex: 1,
          }}
        >
          <Box
            sx={{
              width: "var(--icon-container-lg)",
              height: "var(--icon-container-lg)",
              borderRadius: "var(--radius-control)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              bgcolor: "color-mix(in srgb, var(--primary) 8%, transparent)",
              border:
                "1px solid color-mix(in srgb, var(--primary) 20%, transparent)",
              color: "var(--primary)",
              flexShrink: 0,
              boxShadow:
                "inset 0 0 0 1px color-mix(in srgb, var(--primary) 12%, transparent)",
            }}
          >
            <BeetleIcon
              sx={{
                width: "calc(var(--icon-container-lg) * 0.7)",
                height: "calc(var(--icon-container-lg) * 0.7)",
              }}
            />
          </Box>
          <Box sx={{ minWidth: 0 }}>
            <Typography
              variant="h5"
              sx={{
                fontFamily: "var(--font-display)",
                fontWeight: 700,
                color: "var(--foreground)",
                letterSpacing: "var(--letter-spacing-tight)",
                mb: 0.5,
                fontSize: "var(--font-size-h4)",
                lineHeight: 1.2,
                maxWidth: "min(100%, 17rem)",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {(systemInfo?.product_name || "beetle").toUpperCase()}
              <Box
                component="span"
                sx={{
                  fontFamily: "var(--font-sans)",
                  fontWeight: 600,
                  opacity: 0.92,
                  ml: 0.5,
                }}
              >
                OS
              </Box>
            </Typography>
            <Box
              sx={{
                display: "inline-flex",
                alignItems: "center",
                gap: 0.75,
                px: 1,
                py: 0.25,
                borderRadius: "var(--radius-sm)",
                bgcolor: deviceConnected
                  ? "color-mix(in srgb, var(--semantic-success) 10%, transparent)"
                  : "color-mix(in srgb, var(--muted) 10%, transparent)",
                border: deviceConnected
                  ? "1px solid color-mix(in srgb, var(--semantic-success) 22%, transparent)"
                  : "1px solid color-mix(in srgb, var(--muted) 22%, transparent)",
              }}
            >
              <Box
                sx={{
                  width: "var(--dot-size)",
                  height: "var(--dot-size)",
                  borderRadius: "50%",
                  bgcolor: deviceConnected
                    ? "var(--semantic-success)"
                    : "var(--muted)",
                  boxShadow: deviceConnected
                    ? "0 0 6px color-mix(in srgb, var(--semantic-success) 55%, transparent)"
                    : "none",
                }}
              />
              <Typography
                variant="caption"
                sx={{
                  color: deviceConnected
                    ? "var(--semantic-success)"
                    : "var(--muted)",
                  fontWeight: 600,
                  fontFamily: "var(--font-mono)",
                  fontSize: "0.7rem",
                  letterSpacing: "0.04em",
                }}
              >
                {deviceConnected
                  ? t("device.sysStatusOnline")
                  : t("device.sysStatusOffline")}
              </Typography>
            </Box>
          </Box>
        </Box>

        {deviceConnected && (
          <Button
            variant="outlined"
            size="small"
            startIcon={<RefreshRounded />}
            onClick={() => {
              reloadHealth();
              reloadChannelConnectivity();
            }}
            disabled={healthLoading || channelLoading}
            sx={{
              borderRadius: "var(--radius-control)",
              flexShrink: 0,
              bgcolor: "color-mix(in srgb, var(--card) 72%, transparent)",
              backdropFilter: "blur(10px)",
            }}
          >
            {t("device.channelRefresh")}
          </Button>
        )}
      </Box>

      <Box
        sx={{
          px: 2.5,
          pb: 2.5,
          pt: 2,
          display: "flex",
          flexDirection: "column",
          gap: 2,
          position: "relative",
          zIndex: 1,
        }}
      >
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
        />
        <Box sx={{ display: "flex", gap: 1.5, mt: 1 }}>
          <Button
            variant="contained"
            onClick={handleSave}
            fullWidth
            sx={{ borderRadius: "var(--radius-control)" }}
          >
            {t("device.save")}
          </Button>
          <Button
            variant="outlined"
            onClick={handleProbe}
            disabled={probeStatus === "checking"}
            fullWidth
            sx={{ borderRadius: "var(--radius-control)" }}
          >
            {probeStatus === "checking" ? t("device.probing") : t("device.probe")}
          </Button>
        </Box>
      </Box>

      <Box sx={{ flexGrow: 1 }} />

      {/* Hardware Specs / LEDs (Only show if connected) */}
      {deviceConnected && healthData && resourceData && (
        <Box
          sx={{
            display: "grid",
            gridTemplateColumns: {
              xs: "1fr 1fr",
              lg: "repeat(4, 1fr)",
            },
            gap: "1px",
            bgcolor: "color-mix(in srgb, var(--border) 30%, transparent)",
            borderTop: "1px solid color-mix(in srgb, var(--border) 30%, transparent)",
            position: "relative",
            zIndex: 1,
          }}
        >
          <Box sx={{ bgcolor: "var(--card)", p: 2, display: "flex", flexDirection: "column", gap: 0.5 }}>
            <Typography variant="caption" component="div" sx={UI_LABEL_SECONDARY_SX}>
              {t("device.systemStatusWifiSta")}
            </Typography>
            <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
              <Box sx={{ width: 6, height: 6, borderRadius: "50%", bgcolor: healthData.wifi === "connected" ? "var(--semantic-success)" : "var(--semantic-warning)", boxShadow: healthData.wifi === "connected" ? "0 0 8px var(--semantic-success)" : "none" }} />
              <Typography variant="body2" sx={{ color: "var(--foreground)", fontFamily: "var(--font-mono)", fontSize: "0.8rem", fontWeight: 500 }}>
                {wifiStaLabel(healthData.wifi, t)}
              </Typography>
            </Box>
          </Box>

          <Box sx={{ bgcolor: "var(--card)", p: 2, display: "flex", flexDirection: "column", gap: 0.5 }}>
            <Typography variant="caption" component="div" sx={UI_LABEL_SECONDARY_SX}>
              {t("device.systemStatusDisplayAvailable")}
            </Typography>
            <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
              <Box sx={{ width: 6, height: 6, borderRadius: "50%", bgcolor: healthData.display?.available === true ? "var(--semantic-success)" : "var(--muted)", boxShadow: healthData.display?.available === true ? "0 0 8px var(--semantic-success)" : "none" }} />
              <Typography variant="body2" sx={{ color: "var(--foreground)", fontFamily: "var(--font-mono)", fontSize: "0.8rem", fontWeight: 500 }}>
                {yesNo(healthData.display?.available, t)}
              </Typography>
            </Box>
          </Box>

          <Box sx={{ bgcolor: "var(--card)", p: 2, display: "flex", flexDirection: "column", gap: 0.5 }}>
            <Typography variant="caption" component="div" sx={UI_LABEL_SECONDARY_SX}>
              {t("device.systemStatusPressure")}
            </Typography>
            <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
              <Box sx={{ width: 6, height: 6, borderRadius: "50%", bgcolor: pressureColor(resourceData.pressure), boxShadow: `0 0 8px ${pressureColor(resourceData.pressure)}` }} />
              <Typography variant="body2" sx={{ color: "var(--foreground)", fontFamily: "var(--font-mono)", fontSize: "0.8rem", fontWeight: 500 }}>
                {pressureLabel}
              </Typography>
            </Box>
          </Box>

          <Box sx={{ bgcolor: "var(--card)", p: 2, display: "flex", flexDirection: "column", gap: 0.5 }}>
            <Typography variant="caption" component="div" sx={UI_LABEL_SECONDARY_SX}>
              {t("device.deviceInfoAudio")}
            </Typography>
            <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
              <Box sx={{ width: 6, height: 6, borderRadius: "50%", bgcolor: healthData.audio?.duplex_profile && healthData.audio.duplex_profile !== "unavailable" ? "var(--semantic-success)" : "var(--muted)", boxShadow: healthData.audio?.duplex_profile && healthData.audio.duplex_profile !== "unavailable" ? "0 0 8px var(--semantic-success)" : "none" }} />
              <Typography variant="body2" sx={{ color: "var(--foreground)", fontFamily: "var(--font-mono)", fontSize: "0.8rem", fontWeight: 500 }}>
                {audioProfileLabel}
              </Typography>
            </Box>
          </Box>

          {audioCapabilityRows.map((item) => (
            <Box
              key={item.label}
              sx={{ bgcolor: "var(--card)", p: 2, display: "flex", flexDirection: "column", gap: 0.5 }}
            >
              <Typography variant="caption" component="div" sx={UI_LABEL_SECONDARY_SX}>
                {item.label}
              </Typography>
              <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                <Box
                  sx={{
                    width: 6,
                    height: 6,
                    borderRadius: "50%",
                    bgcolor: item.active ? "var(--semantic-success)" : "var(--muted)",
                    boxShadow: item.active ? "0 0 8px var(--semantic-success)" : "none",
                  }}
                />
                <Typography variant="body2" sx={{ color: "var(--foreground)", fontFamily: "var(--font-mono)", fontSize: "0.8rem", fontWeight: 500 }}>
                  {item.value}
                </Typography>
              </Box>
            </Box>
          ))}
        </Box>
      )}
    </Box>
  );

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 3 }}>
      {!deviceConnected ? (
        <Box sx={{ display: "flex", justifyContent: "center", pt: 8 }}>
          <Box sx={{ width: "100%", maxWidth: 440, minHeight: 380 }}>
            {renderGatewayCard()}
          </Box>
        </Box>
      ) : (
        <Box sx={{ display: "flex", flexDirection: "column", gap: 3 }}>
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

          {healthData && resourceData && metricsData && (
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
                gap: 2.5,
                opacity: 1,
                pointerEvents: "auto",
              }}
            >
              {/* Card 1: OS Namecard & Gateway (4 cols) */}
              <Box
                sx={{
                  gridColumn: { xs: "span 4", sm: "span 4", lg: "span 4" },
                  gridRow: { xs: "span 2", lg: "span 2" },
                }}
              >
                {renderGatewayCard()}
              </Box>

              {/* Card 2: Device Details (4 cols) */}
              <Box
                sx={{
                  gridColumn: { xs: "span 4", sm: "span 4", lg: "span 4" },
                  gridRow: { xs: "span 2", lg: "span 2" },
                }}
              >
                <DashboardCard title={t("device.systemStatusGroupResource")} icon={<MemoryRounded />}>
                  <Box
                    sx={{
                      display: "flex",
                      flexDirection: "column",
                      height: "100%",
                    }}
                  >
                    {/* Device Summary Fields */}
                    <Box
                      sx={{
                        display: "flex",
                        flexDirection: "column",
                        gap: 0.5,
                      }}
                    >
                      {deviceSummaryFields.map((field) => (
                        <StatRow
                          key={field.id}
                          label={t(field.labelKey)}
                          value={renderSummaryFieldValue(
                            field.value,
                            field.valueKind,
                          )}
                        />
                      ))}
                    </Box>
                  </Box>
                </DashboardCard>
              </Box>

              {/* Channel Connectivity (4 cols) */}
              <Box
                sx={{
                  gridColumn: { xs: "span 4", sm: "span 8", lg: "span 4" },
                  gridRow: { xs: "span 2", lg: "span 2" },
                }}
              >
                <DashboardCard
                  title={t("device.sectionChannelConnectivity")}
                  icon={<ChatRounded />}
                >
                  <ChannelConnectivityPanel
                    channels={channelList}
                    loading={channelLoading}
                    error={channelError}
                    onRetry={reloadChannelConnectivity}
                    channelLabel={(id) =>
                      t(`device.${channelNameKey[id] ?? id}`)
                    }
                    t={t}
                  />
                </DashboardCard>
              </Box>

              <SystemStatusPanel
                healthData={healthData}
                resourceData={resourceData}
                metricsData={metricsData}
                runtimeKind={runtimeKind}
                t={t}
              />
            </Box>
          )}
        </Box>
      )}
    </Box>
  );
}
