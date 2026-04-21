import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import List from "@mui/material/List";
import ListItem from "@mui/material/ListItem";
import ListItemText from "@mui/material/ListItemText";
import Typography from "@mui/material/Typography";
import {
  InlineAlert,
  PanelStateBlock,
  PageLoadErrorState,
  PanelStateLoading,
  SectionLoadingSkeleton,
  splitPageErrorState,
} from "../components/form";
import { Os3dIcon } from "../components/Os3dIcon";
import { SettingsSection } from "../components/SettingsSection";
import { OS_ICON_DASHBOARD, OS_ICON_NAV } from "../config/osIcons";
import {
  PAGE_STACK_OUTER_SX,
  TEXT_BODY_TERTIARY_SX,
} from "../theme/panelStyles";
import { LAYOUT_TOKENS } from "../config/themeTokens";
import { useDeviceApi } from "../hooks/useDeviceApi";
import type {
  DiagnoseItem,
  HealthData,
  MetricsSnapshotData,
} from "../api/endpoints/system";
import { createAsyncState } from "../types/asyncState";

function kvEntries(obj: object | null | undefined) {
  if (!obj) return [];
  return Object.entries(obj as Record<string, unknown>).filter(
    ([, value]) => value !== undefined && value !== null,
  );
}

function KvList({ items }: { items: Array<[string, unknown]> }) {
  if (!items.length) return null;
  return (
    <List dense disablePadding>
      {items.map(([key, value]) => (
        <ListItem key={key} sx={{ py: 0.5, px: 0 }}>
          <ListItemText
            primary={`${key}: ${String(value)}`}
            slotProps={{
              primary: {
                variant: "body2",
                sx: { fontFamily: "var(--font-mono)" },
              },
            }}
          />
        </ListItem>
      ))}
    </List>
  );
}

export function SystemLogsPage() {
  const { t } = useTranslation();
  const { api, ready, deviceConnected, connectionChecking } = useDeviceApi();
  const [logsState, setLogsState] = useState(
    createAsyncState<{
      health: HealthData | null;
      metrics: MetricsSnapshotData | null;
      diagnose: DiagnoseItem[];
    }>({
      health: null,
      metrics: null,
      diagnose: [],
    }),
  );

  const loadLogs = useCallback(() => {
    if (!ready || !deviceConnected) return;
    setLogsState((prev) => ({ ...prev, loading: true, error: "" }));
    Promise.all([api.system.health(), api.system.metrics(), api.system.diagnose()])
      .then(([healthRes, metricsRes, diagnoseRes]) => {
        const nextHealth = healthRes.ok && healthRes.data ? healthRes.data : null;
        const nextMetrics = metricsRes.ok && metricsRes.data ? metricsRes.data : null;
        const nextDiagnose = diagnoseRes.ok && diagnoseRes.data ? diagnoseRes.data : [];
        const nextError =
          !healthRes.ok
            ? (healthRes.error ?? "")
            : !metricsRes.ok
              ? (metricsRes.error ?? "")
              : !diagnoseRes.ok
                ? (diagnoseRes.error ?? "")
                : "";
        setLogsState({
          loading: false,
          error: nextError,
          data: { health: nextHealth, metrics: nextMetrics, diagnose: nextDiagnose },
        });
      })
      .catch(() =>
        setLogsState((prev) => ({ ...prev, loading: false, error: "config.errorNetwork" })),
      );
  }, [api.system, deviceConnected, ready]);

  useEffect(() => {
    if (!ready || !deviceConnected) {
      queueMicrotask(() => {
        setLogsState(createAsyncState({ health: null, metrics: null, diagnose: [] }));
      });
      return;
    }
    const id = window.setTimeout(() => {
      void loadLogs();
    }, 0);
    return () => window.clearTimeout(id);
  }, [deviceConnected, ready, loadLogs]);

  const severityColor = (s: string) => {
    if (s === "ok") return "var(--semantic-success)";
    if (s === "warn") return "var(--semantic-warning)";
    return "var(--semantic-danger)";
  };
  const showConnectionLoading = ready && connectionChecking && !deviceConnected;
  const showConnectState = !showConnectionLoading && (!ready || !deviceConnected);
  const hasLogData =
    Boolean(logsState.data.health) ||
    Boolean(logsState.data.metrics) ||
    logsState.data.diagnose.length > 0;
  const logsErrorState = splitPageErrorState({
    hasData: hasLogData,
    loading: logsState.loading,
    error: logsState.error,
    suppress: showConnectState || showConnectionLoading,
  });

  return (
    <Box sx={PAGE_STACK_OUTER_SX}>
      <InlineAlert message={logsErrorState.inlineError} onRetry={loadLogs} />
      <SettingsSection
        pinHeader
        surfaceTone={logsState.loading ? "loading" : "default"}
        sx={{ flex: 1, minHeight: 0 }}
        icon={<Os3dIcon src={OS_ICON_NAV["/system-logs"]} />}
        label={t("systemLogs.sectionLogs")}
      >
        {showConnectionLoading ? (
          <PanelStateLoading>
            <SectionLoadingSkeleton />
          </PanelStateLoading>
        ) : showConnectState ? (
          <PanelStateBlock
            tone="neutral"
            icon={<Os3dIcon src={OS_ICON_DASHBOARD.connection} variant="inline" />}
            title={ready ? t("device.connectFirst") : t("device.bannerNeedDevice")}
            description={t("systemLogs.connectDesc")}
          />
        ) : logsState.loading ? (
          <PanelStateLoading>
            <SectionLoadingSkeleton />
          </PanelStateLoading>
        ) : logsErrorState.blockingError ? (
          <PageLoadErrorState
            message={logsErrorState.blockingError}
            onRetry={loadLogs}
          />
        ) : (
          <Box
            sx={{
              display: "flex",
              flexDirection: "column",
              gap: LAYOUT_TOKENS.spacingSectionStack,
            }}
          >
            {logsState.data.health && (
              <Box>
                <Typography
                  variant="caption"
                  sx={{ ...TEXT_BODY_TERTIARY_SX, fontWeight: 700 }}
                >
                  GET /api/health
                </Typography>
                <Box sx={{ mt: 0.75 }}>
                  <Typography variant="caption" sx={TEXT_BODY_TERTIARY_SX}>
                    health
                  </Typography>
                  <KvList
                    items={kvEntries({
                      wifi: logsState.data.health.wifi,
                      last_error: logsState.data.health.last_error ?? "none",
                    })}
                  />
                </Box>
                <Box sx={{ mt: 0.75 }}>
                  <Typography variant="caption" sx={TEXT_BODY_TERTIARY_SX}>
                    metrics
                  </Typography>
                  <KvList items={kvEntries(logsState.data.metrics)} />
                </Box>
              </Box>
            )}
            {logsState.data.diagnose.length > 0 && (
              <Box>
                <Typography
                  variant="caption"
                  sx={{ ...TEXT_BODY_TERTIARY_SX, fontWeight: 700 }}
                >
                  GET /api/diagnose
                </Typography>
                <List dense disablePadding>
                  {logsState.data.diagnose.map((item, i) => (
                    <ListItem
                      key={i}
                      sx={{ py: 0.5, px: 0, alignItems: "flex-start" }}
                    >
                      <Box
                        sx={{
                          width: 8,
                          height: 8,
                          borderRadius: "50%",
                          bgcolor: severityColor(item.severity),
                          mt: 1.2,
                          mr: 1,
                          flexShrink: 0,
                        }}
                      />
                      <ListItemText
                        primary={`[${item.severity}] ${item.category}: ${item.message}`}
                        slotProps={{
                          primary: {
                            variant: "body2",
                            sx: {
                              fontFamily: "var(--font-mono)",
                              fontSize: "var(--font-size-overline)",
                            },
                          },
                        }}
                      />
                    </ListItem>
                  ))}
                </List>
              </Box>
            )}
            {!logsState.data.health &&
            logsState.data.diagnose.length === 0 &&
            !logsState.loading &&
            !logsState.error &&
            ready ? (
              <PanelStateBlock
                tone="neutral"
                icon={<Os3dIcon src={OS_ICON_NAV["/system-logs"]} />}
                title={t("systemLogs.emptyLogs")}
              />
            ) : null}
          </Box>
        )}
      </SettingsSection>
    </Box>
  );
}
