import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import List from "@mui/material/List";
import ListItem from "@mui/material/ListItem";
import ListItemIcon from "@mui/material/ListItemIcon";
import ListItemText from "@mui/material/ListItemText";
import HandymanOutlined from "@mui/icons-material/HandymanOutlined";
import { InlineAlert, SectionLoadingSkeleton } from "../components/form";
import { SettingsSection } from "../components/SettingsSection";
import { ToolGlyph } from "./toolIcons";
import type { ToolInfo } from "../api/endpoints/tools";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { createAsyncState } from "../types/asyncState";
import {
  endpointSupportedByInventory,
  parseRootInventory,
} from "../api/rootInventory";
import {
  SETTINGS_SECTION_LIST_EMPTY_SX,
  SETTINGS_SECTION_LIST_ROW_SX,
} from "../theme/listItemStyles";
import { PAGE_COLUMN_FILL_SX } from "../theme/panelStyles";

export function ToolsPage() {
  const { t } = useTranslation();
  const { api, ready } = useDeviceApi();
  const [state, setState] = useState(
    createAsyncState<ToolInfo[]>([]),
  );

  const load = useCallback(async () => {
    if (!ready) return;
    setState((prev) => ({ ...prev, loading: true, error: "" }));
    const res = await api.tools.list();
    if (res.ok && res.data) {
      setState({ loading: false, error: "", data: res.data });
    } else {
      let nextError = res.error ?? ""
      if (nextError === "Not Found" || nextError === "not found") {
        const probe = await api.device.probe()
        const inventory = probe.ok ? parseRootInventory(probe.data) : null
        if (!endpointSupportedByInventory(inventory, "GET /api/tools")) {
          nextError = t("tools.unsupportedEndpoint")
        }
      }
      setState((prev) => ({
        ...prev,
        loading: false,
        error: nextError,
      }));
    }
  }, [api.device, api.tools, ready, t]);

  useEffect(() => {
    if (!ready) return;
    const id = window.setTimeout(() => {
      void load();
    }, 0);
    return () => window.clearTimeout(id);
  }, [ready, load]);

  return (
    <Box sx={{ ...PAGE_COLUMN_FILL_SX, gap: 2 }}>
      <InlineAlert message={state.error || null} onRetry={load} />
      <SettingsSection
        pinHeader
        sx={{ flex: 1, minHeight: 0 }}
        icon={<HandymanOutlined sx={{ fontSize: "var(--icon-size-md)" }} />}
        label={t("tools.sectionMain")}
        description={t("tools.sectionMainDesc")}
      >
        {state.loading ? (
          <SectionLoadingSkeleton />
        ) : state.error ? null : state.data.length === 0 ? (
          <List dense disablePadding>
            <ListItem sx={SETTINGS_SECTION_LIST_EMPTY_SX}>
              <ListItemText
                primary={t("tools.emptyList")}
                slotProps={{
                  primary: {
                    variant: "body2",
                    sx: {
                      color: "var(--muted)",
                      fontSize: "var(--font-size-caption)",
                    },
                  },
                }}
              />
            </ListItem>
          </List>
        ) : (
          <List
            dense
            disablePadding
            sx={{
              display: "grid",
              gap: 1,
              gridTemplateColumns: {
                xs: "minmax(0, 1fr)",
                sm: "repeat(2, minmax(0, 1fr))",
                lg: "repeat(3, minmax(0, 1fr))",
              },
            }}
          >
            {state.data.map((tool) => (
              <ListItem
                key={tool.name}
                sx={SETTINGS_SECTION_LIST_ROW_SX}
                aria-label={t(tool.i18n_key, { defaultValue: tool.name })}
              >
                <ListItemIcon
                  sx={{
                    minWidth: 40,
                    alignSelf: "center",
                    color:
                      "color-mix(in srgb, var(--primary) 55%, var(--muted))",
                  }}
                >
                  <ToolGlyph
                    name={tool.name}
                    sx={{ fontSize: "var(--icon-size-sm)" }}
                  />
                </ListItemIcon>
                <ListItemText
                  primary={t(tool.i18n_key, { defaultValue: tool.name })}
                  secondary={tool.name}
                  slotProps={{
                    primary: {
                      sx: {
                        fontSize: "var(--font-size-body-sm)",
                        fontWeight: 600,
                        color: "var(--foreground)",
                      },
                    },
                    secondary: {
                      sx: {
                        fontFamily: "var(--font-mono)",
                        fontSize: "var(--font-size-caption)",
                        color: "var(--muted)",
                        mt: 0.25,
                        wordBreak: "break-word",
                      },
                    },
                  }}
                />
              </ListItem>
            ))}
          </List>
        )}
      </SettingsSection>
    </Box>
  );
}
