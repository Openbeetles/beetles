import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import List from "@mui/material/List";
import ListItem from "@mui/material/ListItem";
import ListItemText from "@mui/material/ListItemText";
import Typography from "@mui/material/Typography";
import DescriptionOutlined from "@mui/icons-material/DescriptionOutlined";
import { InlineAlert, SectionLoadingSkeleton } from "../components/form";
import { SettingsSection } from "../components/SettingsSection";
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
  const { api, ready } = useDeviceApi();
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
    if (!ready) return;
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
  }, [api.system, ready]);

  useEffect(() => {
    if (!ready) {
      queueMicrotask(() => {
        setLogsState(createAsyncState({ health: null, metrics: null, diagnose: [] }));
      });
      return;
    }
    const id = window.setTimeout(() => {
      void loadLogs();
    }, 0);
    return () => window.clearTimeout(id);
  }, [ready, loadLogs]);

  const severityColor = (s: string) => {
    if (s === "ok") return "var(--semantic-success)";
    if (s === "warn") return "var(--semantic-warning)";
    return "var(--semantic-danger)";
  };

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 4 }}>
      <InlineAlert message={logsState.error || null} onRetry={loadLogs} />
      
      <SettingsSection
        icon={<DescriptionOutlined sx={{ fontSize: "var(--icon-size-md)" }} />}
        label={t("systemLogs.sectionLogs")}
      >
        {!ready ? (
          <Typography variant="body2" sx={{ color: "var(--muted)" }}>
            {t("device.connectFirst")}
          </Typography>
        ) : logsState.loading ? (
          <SectionLoadingSkeleton />
        ) : (
          <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
            {logsState.data.health && (
              <Box>
                <Typography
                  variant="caption"
                  sx={{ fontWeight: 700, color: "var(--muted)" }}
                >
                  GET /api/health
                </Typography>
                <Box sx={{ mt: 0.75 }}>
                  <Typography variant="caption" sx={{ color: "var(--muted)" }}>
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
                  <Typography variant="caption" sx={{ color: "var(--muted)" }}>
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
                  sx={{ fontWeight: 700, color: "var(--muted)" }}
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
                              fontSize: "0.8125rem",
                            },
                          },
                        }}
                      />
                    </ListItem>
                  ))}
                </List>
              </Box>
            )}
            {!logsState.data.health && logsState.data.diagnose.length === 0 && !logsState.loading && ready && (
              <Typography variant="body2" sx={{ color: "var(--muted)" }}>
                {t("systemLogs.emptyLogs")}
              </Typography>
            )}
          </Box>
        )}
      </SettingsSection>
    </Box>
  );
}
