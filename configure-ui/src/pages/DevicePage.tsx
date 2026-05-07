import { useEffect, useMemo, useRef, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import type { SxProps, Theme } from "@mui/material/styles";
import RefreshRounded from "@mui/icons-material/RefreshRounded";
import { Os3dIcon } from "../components/Os3dIcon";
import { OS_ICON_DASHBOARD } from "../config/osIcons";
import { DeviceAccessCard } from "../components/DeviceAccessCard";
import {
  PageLoadErrorState,
  SettingsRow,
  splitPageErrorState,
} from "../components/form";
import { BeetleIcon } from "../components/BeetleIcon";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { useDevice } from "../hooks/useDevice";
import { useRevealedPassword } from "../hooks/useRevealedPassword";
import { useToast } from "../hooks/useToast";
import {
  type SystemInfoData,
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
import { LAYOUT_TOKENS } from "../config/themeTokens";
import {
  audioProfileLabelKey,
  buildDeviceOperationalStatusKey,
  buildDeviceSummaryFields,
  pressureLabelKey,
} from "./deviceHomeViewModel";
import { loadDeviceStatusBundle } from "./devicePageLoaders";
import {
  CONFIG_PANEL_LOADING_SX,
  DASHBOARD_CARD_BODY_SX,
  DASHBOARD_CARD_HEADER_ROW_SX,
  DASHBOARD_CARD_SURFACE_SX,
  DASHBOARD_HOME_GRID_GAP,
  DASHBOARD_INSET_WELL_BG,
  PAGE_SCROLL_CANVAS_SX,
  TEXT_BODY_TERTIARY_SX,
  TEXT_DASHBOARD_CARD_TITLE_SX,
} from "../theme/panelStyles";
import { useDeviceRuntimeKind } from "../store/deviceStatusStore";
import {
  DEFAULT_DEVICE_BASE_URL,
  deriveDeviceAccessStage,
  normalizeDeviceUrl,
} from "./deviceAccessFlow";
import { enabledChannelLabelKey } from "../types/appConfig";

const DEVICE_PAGE_DIRTY_OWNER = "device-page-connection";

type ConnectionEditorVariant = "dashboard" | "setup";

interface ConnectionEditorProps {
  variant: ConnectionEditorVariant;
  urlValue: string;
  codeValue: string;
  pairingCodeReveal: ReturnType<typeof useRevealedPassword>;
  onUrlChange: (value: string) => void;
  onCodeChange: (value: string) => void;
  onSave: () => void;
  onProbe: () => void;
  probeChecking: boolean;
  baseUrlLabel: string;
  baseUrlPlaceholder: string;
  pairingCodeLabel: string;
  pairingCodePlaceholder: string;
  saveLabel: string;
  probeLabel: string;
  showPairingCode?: boolean;
}

function ConnectionEditor({
  variant,
  urlValue,
  codeValue,
  pairingCodeReveal,
  onUrlChange,
  onCodeChange,
  onSave,
  onProbe,
  probeChecking,
  baseUrlLabel,
  baseUrlPlaceholder,
  pairingCodeLabel,
  pairingCodePlaceholder,
  saveLabel,
  probeLabel,
  showPairingCode = true,
}: ConnectionEditorProps) {
  const isDashboard = variant === "dashboard";

  const actionButtons = (
    <Box
      sx={{
        display: "flex",
        gap: LAYOUT_TOKENS.spacingTitleToContent,
        mt: isDashboard ? 2.5 : 1,
        pt: isDashboard ? 1 : 0,
      }}
    >
      <Button
        variant="contained"
        onClick={onSave}
        fullWidth
        size={isDashboard ? "medium" : "large"}
        sx={{
          borderRadius: "var(--radius-full)",
          py: isDashboard ? 1 : 1.25,
          fontWeight: 600,
          boxShadow: "none",
          "&:hover": { boxShadow: "none" },
        }}
      >
        {saveLabel}
      </Button>
      <Button
        variant="outlined"
        onClick={onProbe}
        disabled={probeChecking}
        fullWidth
        size={isDashboard ? "medium" : "large"}
        sx={{
          borderRadius: "var(--radius-full)",
          py: isDashboard ? 1 : 1.25,
          fontWeight: 600,
        }}
      >
        {probeLabel}
      </Button>
    </Box>
  );

  if (isDashboard) {
    return (
      <Box
        sx={{
          display: "flex",
          flexDirection: "column",
          gap: 0,
          height: "100%",
          justifyContent: "center",
        }}
      >
        <SettingsRow label={baseUrlLabel}>
          <TextField
            hiddenLabel
            placeholder={baseUrlPlaceholder}
            value={urlValue}
            onChange={(e) => onUrlChange(e.target.value)}
            variant="outlined"
            fullWidth
            aria-label={baseUrlLabel}
            slotProps={{
              htmlInput: {
                style: {
                  fontFamily: "var(--font-mono)",
                  fontSize: "var(--font-size-caption)",
                },
              },
            }}
          />
        </SettingsRow>
        {showPairingCode ? (
          <SettingsRow label={pairingCodeLabel} divider={false}>
            <TextField
              hiddenLabel
              placeholder={pairingCodePlaceholder}
              value={codeValue}
              type={pairingCodeReveal.type}
              onChange={(e) => onCodeChange(e.target.value)}
              variant="outlined"
              fullWidth
              aria-label={pairingCodeLabel}
              slotProps={{
                htmlInput: {
                  maxLength: 6,
                  style: {
                    fontFamily: "var(--font-mono)",
                    fontSize: "var(--font-size-caption)",
                    letterSpacing: "0.2em",
                  },
                  ...pairingCodeReveal.inputProps,
                },
              }}
            />
          </SettingsRow>
        ) : null}
        {actionButtons}
      </Box>
    );
  }

  return (
    <Box
      sx={{
        position: "relative",
        zIndex: 1,
        display: "flex",
        flexDirection: "column",
        gap: LAYOUT_TOKENS.spacingFormFields,
      }}
    >
      <TextField
        label={baseUrlLabel}
        placeholder={baseUrlPlaceholder}
        value={urlValue}
        onChange={(e) => onUrlChange(e.target.value)}
        variant="outlined"
        fullWidth
        slotProps={{
          htmlInput: { style: { fontFamily: "var(--font-mono)" } },
        }}
      />
      {showPairingCode ? (
        <TextField
          label={pairingCodeLabel}
          placeholder={pairingCodePlaceholder}
          value={codeValue}
          type={pairingCodeReveal.type}
          onChange={(e) => onCodeChange(e.target.value)}
          variant="outlined"
          fullWidth
          slotProps={{
            htmlInput: {
              maxLength: 6,
              style: {
                fontFamily: "var(--font-mono)",
                letterSpacing: "0.2em",
              },
              ...pairingCodeReveal.inputProps,
            },
          }}
        />
      ) : null}
      {actionButtons}
    </Box>
  );
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
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            gap: LAYOUT_TOKENS.spacingTitleToContent,
            minWidth: 0,
          }}
        >
          {icon && (
            <Box
              sx={{
                width: "var(--icon-size-lg)",
                height: "var(--icon-size-lg)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                flexShrink: 0,
                /** 3D 拟物图标自带 drop-shadow，不再套浅底与 inset 边框 */
              }}
            >
              {icon}
            </Box>
          )}
          <Typography variant="subtitle2" sx={TEXT_DASHBOARD_CARD_TITLE_SX}>
            {title}
          </Typography>
        </Box>
        {action}
      </Box>
      <Box sx={DASHBOARD_CARD_BODY_SX}>{children}</Box>
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
        gap: LAYOUT_TOKENS.spacingTitleToContent,
        py: 1.1,
      }}
    >
      <Typography
        variant="body2"
        sx={{
          fontSize: "var(--font-size-caption)",
          fontWeight: 500,
          color: "var(--text-secondary)",
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
          color: "var(--text-primary)",
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
  const { setDirtyFor, clearDirtyFor } = useUnsaved();
  const { baseUrl, pairingCode, setBaseUrl, setPairingCode } = useDevice();
  const runtimeKind = useDeviceRuntimeKind();
  const [urlInput, setUrlInput] = useState(baseUrl || DEFAULT_DEVICE_BASE_URL);
  const [codeInput, setCodeInput] = useState(pairingCode);
  const pairingCodeReveal = useRevealedPassword();
  const { api, appMode, canAccessProtectedApis } = useDeviceApi();
  const accessStage = deriveDeviceAccessStage(appMode);
  const { showToast } = useToast();
  const [probeStatus, setProbeStatus] = useState<
    "idle" | "checking" | "ok" | "fail"
  >("idle");
  const [systemInfo, setSystemInfo] = useState<SystemInfoData | null>(null);
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
  const healthLoadRequestIdRef = useRef(0);
  useEffect(() => {
    if (deviceSessionKey === prevDeviceSessionKeyRef.current) return;
    prevDeviceSessionKeyRef.current = deviceSessionKey;
    const nextUrl = baseUrl || DEFAULT_DEVICE_BASE_URL;
    const nextCode = pairingCode ?? "";
    queueMicrotask(() => {
      setUrlInput(nextUrl);
      setCodeInput(nextCode);
      setSystemInfo(null);
      setHealthData(null);
      setResourceData(null);
      setMetricsData(null);
      setHealthLoading(false);
      setHealthError("");
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
  const dashboardReady = Boolean(healthData && resourceData && metricsData);
  const dashboardErrorState = splitPageErrorState({
    hasData: dashboardReady,
    loading: healthLoading,
    error: healthError,
    suppress: !canAccessProtectedApis,
  });

  useEffect(() => {
    setDirtyFor(
      DEVICE_PAGE_DIRTY_OWNER,
      accessStage === "ready" && connectionDraftDirty,
    );
  }, [accessStage, connectionDraftDirty, setDirtyFor]);

  useEffect(() => {
    return () => clearDirtyFor(DEVICE_PAGE_DIRTY_OWNER);
  }, [clearDirtyFor]);

  const handleSaveAccess = () => {
    const url = normalizeDeviceUrl(urlInput);
    setBaseUrl(url);
    setPairingCode(codeInput.trim());
    showToast(t("common.saveOk"), { variant: "success" });
  };

  const handleProbe = async () => {
    const url = normalizeDeviceUrl(urlInput);
    setProbeStatus("checking");
    const res = await api.device.probe(url);
    if (res.ok) {
      setProbeStatus("ok");
      showToast(t("device.probeOk"), { variant: "success" });
    } else {
      setProbeStatus("fail");
      showToast(`${t("device.probeFail")}: ${res.error || t("common.error")}`, {
        variant: "error",
      });
    }
  };

  useEffect(() => {
    if (!canAccessProtectedApis || !baseUrl?.trim()) return;
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
  }, [api.system, canAccessProtectedApis, baseUrl, pairingCode]);

  const runHealthLoad = useCallback(
    ({
      notify,
      clearError,
    }: {
      notify: boolean;
      clearError: boolean;
    }) => {
      if (!canAccessProtectedApis || !baseUrl?.trim()) return undefined;
      const requestId = ++healthLoadRequestIdRef.current;
      if (clearError) setHealthError("");
      setHealthLoading(true);
      let active = true;
      void loadDeviceStatusBundle({
        health: api.system.health,
        resource: api.system.resource,
        metrics: api.system.metrics,
      }).then((result) => {
        if (!active || healthLoadRequestIdRef.current !== requestId) return;
        setHealthLoading(false);
        if (result.ok) {
          setHealthError("");
          setHealthData(result.data.health);
          setResourceData(result.data.resource);
          setMetricsData(result.data.metrics);
          return;
        }
        setHealthError(result.error);
        if (notify) {
          showToast(`${t("device.systemStatusLoadFail")}: ${result.error}`, {
            variant: "error",
          });
        }
      });
      return () => {
        active = false;
      };
    },
    [api.system, baseUrl, canAccessProtectedApis, showToast, t],
  );

  const reloadHealth = useCallback(() => {
    void runHealthLoad({ notify: true, clearError: true });
  }, [runHealthLoad]);

  useEffect(() => {
    if (!canAccessProtectedApis || !baseUrl?.trim()) return;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- initial device sync intentionally enters loading as soon as the session becomes reachable
    return runHealthLoad({ notify: false, clearError: false });
  }, [canAccessProtectedApis, baseUrl, runHealthLoad]);

  const pressureLabel =
    pressureLabelKey(resourceData?.pressure) != null
      ? t(pressureLabelKey(resourceData?.pressure) as string)
      : t("common.na");
  const audioProfileLabel =
    audioProfileLabelKey(healthData?.audio?.duplex_profile) != null
      ? t(audioProfileLabelKey(healthData?.audio?.duplex_profile) as string)
      : t("common.na");
  const currentChannelId = healthData?.current_channel?.id ?? "";
  const currentChannelLabel = t(enabledChannelLabelKey(currentChannelId));
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
      <Box
        sx={{
          position: "relative",
          zIndex: 1,
          display: "flex",
          justifyContent: "space-between",
          alignItems: "flex-start",
          flexWrap: "wrap",
          gap: LAYOUT_TOKENS.spacingSectionStack,
        }}
      >
        <Box sx={{ display: "flex", gap: 3, alignItems: "center" }}>
          <Box
            sx={{
              width: 72,
              height: 72,
              borderRadius: "var(--radius-card)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              bgcolor: "color-mix(in srgb, var(--primary) 12%, transparent)",
              color: "var(--primary)",
              boxShadow: [
                "var(--os3d-pedestal-lift-stack)",
                "0 10px 22px color-mix(in srgb, var(--primary) 11%, transparent)",
              ].join(", "),
              flexShrink: 0,
            }}
          >
            <BeetleIcon motion="idle" sx={{ width: 40, height: 40 }} />
          </Box>
          <Box sx={{ minWidth: 0 }}>
            <Typography
              variant="h3"
              sx={{
                fontFamily: "var(--font-brand)",
                fontWeight: 800,
                color: "var(--text-primary)",
                letterSpacing: 0,
                lineHeight: 1.1,
                mb: 1.5,
                whiteSpace: "nowrap",
                overflow: "hidden",
                textOverflow: "ellipsis",
              }}
            >
              {systemInfo?.product_name?.trim()
                ? systemInfo.product_name.toUpperCase()
                : (() => {
                    const full = t("app.name");
                    const head = full.replace(/\s*OS\s*$/i, "").trim();
                    return (
                      <>
                        {head.toUpperCase()}
                        <Box
                          component="span"
                          sx={{ color: "var(--primary)", ml: 1.5, fontWeight: 700 }}
                        >
                          {" OS"}
                        </Box>
                      </>
                    );
                  })()}
            </Typography>
            <Box
              sx={{
                display: "flex",
                gap: LAYOUT_TOKENS.spacingTitleToContent,
                alignItems: "center",
                flexWrap: "wrap",
              }}
            >
              <Box
                sx={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: LAYOUT_TOKENS.spacingInlineTight,
                  px: 1.25,
                  py: 0.5,
                  borderRadius: "var(--radius-full)",
                  bgcolor:
                    "color-mix(in srgb, var(--semantic-success) 12%, transparent)",
                }}
              >
                <Box
                  sx={{
                    width: 8,
                    height: 8,
                    borderRadius: "50%",
                    bgcolor: "var(--semantic-success)",
                    boxShadow: "0 0 8px var(--semantic-success)",
                  }}
                />
                <Typography
                  variant="caption"
                  sx={{
                    color: "var(--semantic-success)",
                    fontWeight: 700,
                    letterSpacing: "0.05em",
                    lineHeight: 1,
                  }}
                >
                  {t("device.sysStatusOnline").toUpperCase()}
                </Typography>
              </Box>
              {systemInfo?.lan_ip && (
                <Typography
                  variant="caption"
                  sx={{
                    fontFamily: "var(--font-mono)",
                    color: "var(--text-secondary)",
                    bgcolor: DASHBOARD_INSET_WELL_BG,
                    px: 1.25,
                    py: 0.5,
                    borderRadius: "var(--radius-full)",
                    fontWeight: 600,
                    lineHeight: 1,
                  }}
                >
                  {systemInfo.lan_ip}
                </Typography>
              )}
              {systemInfo?.firmware_version && (
                <Typography
                  variant="caption"
                  sx={{
                    fontFamily: "var(--font-mono)",
                    color: "var(--text-secondary)",
                    bgcolor: DASHBOARD_INSET_WELL_BG,
                    px: 1.25,
                    py: 0.5,
                    borderRadius: "var(--radius-full)",
                    fontWeight: 600,
                    lineHeight: 1,
                  }}
                >
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
          onClick={reloadHealth}
          disabled={healthLoading}
          sx={{
            borderRadius: "var(--radius-full)",
            bgcolor: "color-mix(in srgb, var(--card) 50%, transparent)",
            backdropFilter: "blur(var(--shell-chrome-blur))",
            WebkitBackdropFilter: "blur(var(--shell-chrome-blur))",
          }}
        >
          {t("device.refreshDashboard")}
        </Button>
      </Box>

      <Box sx={{ flexGrow: 1, minHeight: { xs: 24, sm: 28 } }} />

      {/* Hardware LEDs Strip */}
      {healthData && resourceData && (
        <Box
          sx={{
            position: "relative",
            zIndex: 1,
            display: "flex",
            flexWrap: "wrap",
            gap: LAYOUT_TOKENS.spacingTitleToContent,
            mt: { xs: 3, md: 3.5 },
          }}
        >
          {[
            {
              label: t("device.systemStatusWifiSta"),
              value: wifiStaLabel(healthData.network_status?.sta_connected, t),
              color:
                healthData.network_status?.sta_connected
                  ? "var(--semantic-success)"
                  : "var(--semantic-warning)",
              active: true,
            },
            {
              label: t("device.systemStatusCurrentChannel"),
              value: currentChannelLabel,
              color: currentChannelId
                ? "var(--semantic-success)"
                : "var(--text-tertiary)",
              active: Boolean(currentChannelId),
            },
            {
              label: t("device.systemStatusDisplayAvailable"),
              value: yesNo(healthData.display?.available, t),
              color: healthData.display?.available
                ? "var(--semantic-success)"
                : "var(--text-tertiary)",
              active: healthData.display?.available,
            },
            {
              label: t("device.systemStatusPressure"),
              value: pressureLabel,
              color: pressureColor(resourceData.pressure),
              active: true,
            },
            {
              label: t("device.deviceInfoAudio"),
              value: audioProfileLabel,
              color:
                healthData.audio?.duplex_profile &&
                healthData.audio.duplex_profile !== "unavailable"
                  ? "var(--semantic-success)"
                  : "var(--text-tertiary)",
              active:
                healthData.audio?.duplex_profile &&
                healthData.audio.duplex_profile !== "unavailable",
            },
            ...audioCapabilityRows.map((r) => ({
              label: r.label,
              value: r.value,
              color: r.active ? "var(--semantic-success)" : "var(--text-tertiary)",
              active: r.active,
            })),
          ].map((led, idx) => (
            <Box
              key={idx}
              sx={{
                display: "flex",
                alignItems: "center",
                gap: LAYOUT_TOKENS.spacingInlineTight,
                px: 1.5,
                py: 1.1,
                borderRadius: "var(--radius-chip)",
                bgcolor: DASHBOARD_INSET_WELL_BG,
                flex: "1 1 148px",
                minWidth: 132,
              }}
            >
              <Box
                sx={{
                  width: 8,
                  height: 8,
                  borderRadius: "50%",
                  bgcolor: led.color,
                  boxShadow: led.active ? `0 0 10px ${led.color}` : "none",
                  flexShrink: 0,
                }}
              />
              <Box sx={{ minWidth: 0 }}>
                <Typography
                  variant="caption"
                  sx={{
                    ...TEXT_BODY_TERTIARY_SX,
                    display: "block",
                    fontSize: "var(--font-size-label)",
                    textTransform: "uppercase",
                    letterSpacing: "0.06em",
                    lineHeight: 1,
                    mb: 0.45,
                    whiteSpace: "nowrap",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                  }}
                >
                  {led.label}
                </Typography>
                <Typography
                  variant="body2"
                  sx={{
                    fontFamily: "var(--font-mono)",
                    fontSize: "var(--font-size-data-value)",
                    fontWeight: 600,
                    color: "var(--text-primary)",
                    lineHeight: 1.15,
                    whiteSpace: "nowrap",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                  }}
                >
                  {led.value}
                </Typography>
              </Box>
            </Box>
          ))}
        </Box>
      )}
    </Box>
  );

  const renderDashboardLoadingState = () => (
    <Box
      sx={{
        ...CONFIG_PANEL_LOADING_SX,
        minHeight: 220,
        display: "flex",
        flexDirection: "column",
        justifyContent: "center",
        gap: 2,
        px: { xs: 2.5, sm: 3 },
        py: { xs: 3, sm: 4 },
      }}
    >
      <Typography variant="subtitle2" sx={TEXT_DASHBOARD_CARD_TITLE_SX}>
        {t("device.pageTitle")}
      </Typography>
      <SectionLoadProgress loading={healthLoading} />
    </Box>
  );

  const renderDashboardLoadErrorState = () => (
    <Box
      sx={{
        ...CONFIG_PANEL_LOADING_SX,
        minHeight: 220,
        display: "flex",
        flexDirection: "column",
        justifyContent: "center",
        gap: 2,
        px: { xs: 2.5, sm: 3 },
        py: { xs: 3, sm: 4 },
      }}
    >
      <Typography variant="subtitle2" sx={TEXT_DASHBOARD_CARD_TITLE_SX}>
        {t("device.pageTitle")}
      </Typography>
      <PageLoadErrorState
        message={dashboardErrorState.blockingError}
        onRetry={reloadHealth}
      />
    </Box>
  );

  const renderConnectionCard = () => (
    <DashboardCard
      title={t("device.sectionConnection")}
      icon={<Os3dIcon src={OS_ICON_DASHBOARD.connection} variant="tile" />}
      sx={{ height: "100%" }}
    >
      <ConnectionEditor
        variant="dashboard"
        urlValue={urlInput}
        codeValue={codeInput}
        pairingCodeReveal={pairingCodeReveal}
        onUrlChange={setUrlInput}
        onCodeChange={setCodeInput}
        onSave={handleSaveAccess}
        onProbe={handleProbe}
        probeChecking={probeStatus === "checking"}
        baseUrlLabel={t("device.baseUrlLabel")}
        baseUrlPlaceholder={t("device.baseUrlPlaceholder")}
        pairingCodeLabel={t("device.pairingCodeLabel")}
        pairingCodePlaceholder={t("device.pairingCodePlaceholder")}
        saveLabel={t("device.save")}
        probeLabel={
          probeStatus === "checking" ? t("device.probing") : t("device.probe")
        }
      />
    </DashboardCard>
  );

  if (accessStage !== "ready") {
    return <DeviceAccessCard />;
  }

  return (
    <>
      <Box
        data-app-scroll-region
        sx={{
          ...PAGE_SCROLL_CANVAS_SX,
          gap: DASHBOARD_HOME_GRID_GAP,
          py: { xs: 2, sm: 2.5, lg: 3 },
          boxSizing: "border-box",
        }}
      >
        {!dashboardReady ? (
          <Box
            sx={{
              display: "flex",
              flexDirection: "column",
              gap: DASHBOARD_HOME_GRID_GAP,
            }}
          >
            {dashboardErrorState.blockingError
              ? renderDashboardLoadErrorState()
              : renderDashboardLoadingState()}
          </Box>
        ) : (
          <Box
            sx={{
              display: "flex",
              flexDirection: "column",
              gap: DASHBOARD_HOME_GRID_GAP,
            }}
          >
            {dashboardErrorState.inlineError ? (
              <PageLoadErrorState
                message={dashboardErrorState.inlineError}
                onRetry={reloadHealth}
              />
            ) : null}
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
              <Box
                sx={{
                  gridColumn: { xs: "span 4", sm: "span 8", lg: "span 8" },
                  gridRow: { xs: "span 2", sm: "span 2", lg: "span 2" },
                }}
              >
                {renderHeroCard()}
              </Box>

              <Box
                sx={{
                  gridColumn: { xs: "span 4", sm: "span 4", lg: "span 4" },
                  gridRow: { xs: "span 2", sm: "span 2", lg: "span 2" },
                }}
              >
                {renderConnectionCard()}
              </Box>

              {/* Row 2: Device details + system status */}
              <Box
                sx={{
                  gridColumn: { xs: "span 4", sm: "span 4", lg: "span 4" },
                  gridRow: { xs: "span 2", lg: "span 2" },
                }}
              >
                <DashboardCard
                  title={t("device.sectionDeviceInfo")}
                  icon={
                    <Os3dIcon
                      src={OS_ICON_DASHBOARD.deviceInfo}
                      variant="tile"
                    />
                  }
                >
                  <Box
                    sx={{
                      display: "flex",
                      flexDirection: "column",
                      gap: 0,
                      "& > *:not(:last-of-type)": {
                        borderBottom: "var(--divider-row)",
                      },
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
                </DashboardCard>
              </Box>

              <SystemStatusPanel
                healthData={healthData!}
                resourceData={resourceData!}
                metricsData={metricsData!}
                systemInfo={systemInfo}
                runtimeKind={runtimeKind}
                t={t}
              />
            </Box>
          </Box>
        )}
      </Box>
    </>
  );
}
