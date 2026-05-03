import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import List from "@mui/material/List";
import ListItem from "@mui/material/ListItem";
import ListItemIcon from "@mui/material/ListItemIcon";
import ListItemText from "@mui/material/ListItemText";
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
import { OS_ICON_NAV } from "../config/osIcons";
import { ToolGlyph } from "./toolIcons";
import type { ToolInfo } from "../api/endpoints/tools";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { createAsyncState } from "../types/asyncState";
import {
  apiResultIndicatesUnsupportedEndpoint,
  endpointSupportedByInventory,
  parseRootInventory,
} from "../api/rootInventory";
import { SETTINGS_LIST_ROW_PLATE_SX } from "../theme/listItemStyles";
import { PAGE_STACK_OUTER_SX } from "../theme/panelStyles";
import { LAYOUT_TOKENS } from "../config/themeTokens";
import { translateApiError } from "../i18n/apiErrors";

export function ToolsPage() {
  const { t } = useTranslation();
  const { api, ready, deviceConnected, connectionChecking } = useDeviceApi();
  const [state, setState] = useState(
    createAsyncState<ToolInfo[]>([]),
  );
  const [unsupportedEndpoint, setUnsupportedEndpoint] = useState(false);

  const load = useCallback(async () => {
    if (!ready || !deviceConnected) return;
    setState((prev) => ({ ...prev, loading: true, error: "" }));
    setUnsupportedEndpoint(false);
    const res = await api.tools.list();
    if (res.ok && res.data) {
      setUnsupportedEndpoint(false);
      setState({ loading: false, error: "", data: res.data });
    } else {
      let nextError = res.error ?? "";
      let nextUnsupported = false;
      if (
        apiResultIndicatesUnsupportedEndpoint(res) ||
        res.errorKey === "common.not_found" ||
        res.status === 404
      ) {
        const probe = await api.device.probe();
        const inventory = probe.ok ? parseRootInventory(probe.data) : null;
        if (
          apiResultIndicatesUnsupportedEndpoint(res) ||
          !endpointSupportedByInventory(inventory, "GET /api/tools")
        ) {
          nextUnsupported = true;
          nextError = "";
        }
      }
      setUnsupportedEndpoint(nextUnsupported);
      setState((prev) => ({
        ...prev,
        loading: false,
        error: nextUnsupported ? "" : translateApiError(t, nextError, "common.error"),
      }));
    }
  }, [api.device, api.tools, deviceConnected, ready, t]);

  useEffect(() => {
    if (!ready || !deviceConnected) {
      queueMicrotask(() => {
        setUnsupportedEndpoint(false);
        setState(createAsyncState<ToolInfo[]>([]));
      });
      return;
    }
    const id = window.setTimeout(() => {
      void load();
    }, 0);
    return () => window.clearTimeout(id);
  }, [deviceConnected, ready, load]);

  const showConnectionLoading = ready && connectionChecking && !deviceConnected;
  const showConnectState = !showConnectionLoading && (!ready || !deviceConnected);
  const listErrorState = splitPageErrorState({
    hasData: state.data.length > 0,
    loading: state.loading,
    error: state.error,
    suppress: showConnectState || showConnectionLoading || unsupportedEndpoint,
  });

  return (
    <Box sx={PAGE_STACK_OUTER_SX}>
      <InlineAlert message={listErrorState.inlineError} onRetry={load} />
      <SettingsSection
        pinHeader
        surfaceTone={state.loading ? "loading" : "default"}
        sx={{ flex: 1, minHeight: 0 }}
        icon={<Os3dIcon src={OS_ICON_NAV["/tools"]} />}
        label={t("tools.sectionMain")}
        description={t("tools.sectionMainDesc")}
      >
        {showConnectionLoading ? (
          <PanelStateLoading>
            <SectionLoadingSkeleton />
          </PanelStateLoading>
        ) : showConnectState ? (
          <PanelStateBlock
            tone="neutral"
            icon={<Os3dIcon src={OS_ICON_NAV["/tools"]} variant="inline" />}
            title={ready ? t("device.connectFirst") : t("device.bannerNeedDevice")}
            description={t("tools.connectDesc")}
          />
        ) : state.loading ? (
          <PanelStateLoading>
            <SectionLoadingSkeleton />
          </PanelStateLoading>
        ) : unsupportedEndpoint ? (
          <PanelStateBlock
            tone="neutral"
            icon={<Os3dIcon src={OS_ICON_NAV["/tools"]} variant="inline" />}
            title={t("tools.unsupportedTitle")}
            description={t("tools.unsupportedDesc")}
          />
        ) : listErrorState.blockingError ? (
          <PageLoadErrorState
            message={listErrorState.blockingError}
            onRetry={load}
          />
        ) : state.data.length === 0 ? (
          <PanelStateBlock
            tone="neutral"
            icon={<Os3dIcon src={OS_ICON_NAV["/tools"]} />}
            title={t("tools.emptyList")}
          />
        ) : (
          <List
            dense
            disablePadding
            sx={{
              display: "grid",
              gap: LAYOUT_TOKENS.spacingInlineTight,
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
                sx={SETTINGS_LIST_ROW_PLATE_SX}
                aria-label={t(tool.i18n_key, { defaultValue: tool.name })}
              >
                <ListItemIcon
                  sx={{
                    minWidth: LAYOUT_TOKENS.toolsListIconSlotPx,
                    alignSelf: "center",
                    display: "flex",
                    alignItems: "center",
                  }}
                >
                  <ToolGlyph name={tool.name} />
                </ListItemIcon>
                <ListItemText
                  primary={t(tool.i18n_key, { defaultValue: tool.name })}
                  slotProps={{
                    primary: {
                      sx: {
                        fontSize: "var(--font-size-body-sm)",
                        fontWeight: 600,
                        color: "var(--text-primary)",
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
