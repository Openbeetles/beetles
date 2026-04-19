import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Chip from "@mui/material/Chip";
import Stack from "@mui/material/Stack";
import ToggleButton from "@mui/material/ToggleButton";
import ToggleButtonGroup from "@mui/material/ToggleButtonGroup";
import Typography from "@mui/material/Typography";
import Button from "@mui/material/Button";
import AddRounded from "@mui/icons-material/AddRounded";
import ChevronRightRounded from "@mui/icons-material/ChevronRightRounded";
import { AccountDetailDialog } from "../components/AccountDetailDialog";
import {
  InlineAlert,
  PanelStateBlock,
  PanelStateLoading,
  SectionLoadingSkeleton,
} from "../components/form";
import { Os3dIcon } from "../components/Os3dIcon";
import { SettingsSection } from "../components/SettingsSection";
import {
  endpointSupportedByInventory,
  parseRootInventory,
} from "../api/rootInventory";
import { OS_ICON_NAV } from "../config/osIcons";
import { LAYOUT_TOKENS } from "../config/themeTokens";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { localizeAccountProviderName } from "../i18n/providerDisplay";
import type { AccountCapability, AccountSummary } from "../types/accountConfig";
import {
  CONFIG_PANEL_SX,
  PAGE_STACK_OUTER_SX,
} from "../theme/panelStyles";

type CapabilityFilter = "all" | AccountCapability;

const CAPABILITIES: AccountCapability[] = [
  "mail",
  "calendar",
  "documents",
  "contacts_directory",
];

const ACCOUNT_FILTER_GROUP_SX = {
  flexWrap: "wrap",
  gap: 0.375,
  p: 0.25,
  borderRadius: "var(--radius-control)",
  bgcolor: "color-mix(in srgb, var(--surface) 76%, var(--card))",
  backgroundImage:
    "linear-gradient(180deg, color-mix(in srgb, #fff 14%, transparent) 0%, transparent 100%)",
  boxShadow: [
    "inset 0 1px 0 color-mix(in srgb, #fff 44%, transparent)",
    "inset 0 -1px 0 color-mix(in srgb, var(--foreground) 3%, transparent)",
  ].join(", "),
  "& .MuiToggleButtonGroup-grouped": {
    minHeight: LAYOUT_TOKENS.buttonMinHeightSmall,
    px: 1.25,
    py: 0,
    border: "1px solid transparent",
    borderRadius: "calc(var(--radius-control) - 1px) !important",
    backgroundColor: "transparent",
    backgroundImage: "none",
    boxShadow: "none",
    fontSize: "var(--font-size-caption)",
    fontWeight: 600,
    lineHeight: 1,
    color: "color-mix(in srgb, var(--foreground) 72%, transparent)",
    "&:hover": {
      backgroundColor: "color-mix(in srgb, var(--foreground) 3%, var(--card))",
      color: "var(--foreground)",
      boxShadow: "none",
    },
    "&.Mui-selected": {
      backgroundColor: "color-mix(in srgb, var(--primary) 6%, var(--card))",
      backgroundImage:
        "linear-gradient(180deg, color-mix(in srgb, #fff 18%, transparent) 0%, transparent 100%)",
      color: "var(--primary)",
      borderColor: "color-mix(in srgb, var(--primary) 16%, transparent)",
      boxShadow: [
        "inset 0 1px 0 color-mix(in srgb, #fff 54%, transparent)",
        "0 1px 2px color-mix(in srgb, var(--foreground) 3.5%, transparent)",
      ].join(", "),
      "&:hover": {
        backgroundColor: "color-mix(in srgb, var(--primary) 8%, var(--card))",
      },
    },
  },
} as const;

type AccountsDialogState =
  | { kind: "closed" }
  | { kind: "create" }
  | { kind: "detail"; accountKey: string };

function readinessColor(
  r: AccountSummary["readiness"],
): "default" | "success" | "warning" | "error" {
  switch (r) {
    case "ready":
      return "success";
    case "needs_configuration":
    case "ready_for_probe":
      return "warning";
    default:
      return "default";
  }
}

export function AccountsPage() {
  const { t } = useTranslation();
  const { api, ready, hasPairing } = useDeviceApi();
  const [capFilter, setCapFilter] = useState<CapabilityFilter>("all");
  const [items, setItems] = useState<AccountSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [unsupportedEndpoint, setUnsupportedEndpoint] = useState(false);
  const [dialog, setDialog] = useState<AccountsDialogState>({ kind: "closed" });

  const filters = useMemo(() => {
    if (capFilter === "all") return undefined;
    return { capability: capFilter };
  }, [capFilter]);

  const load = useCallback(async () => {
    if (!ready) return;
    setLoading(true);
    setError("");
    setUnsupportedEndpoint(false);
    const res = await api.config.accounts.list(filters);
    if (res.ok && res.data) {
      setItems(res.data.items);
    } else {
      let nextError = res.error ?? t("accounts.loadFailed");
      let nextUnsupported = false;
      if (nextError === "Not Found" || nextError === "not found") {
        const probe = await api.device.probe();
        const inventory = probe.ok ? parseRootInventory(probe.data) : null;
        if (!endpointSupportedByInventory(inventory, "GET /api/config/accounts")) {
          nextUnsupported = true;
          nextError = "";
        }
      }
      setUnsupportedEndpoint(nextUnsupported);
      setError(nextError);
      setItems([]);
    }
    setLoading(false);
  }, [api.config.accounts, api.device, filters, ready, t]);

  useEffect(() => {
    if (!ready) return;
    const id = window.setTimeout(() => {
      void load();
    }, 0);
    return () => window.clearTimeout(id);
  }, [ready, load]);

  const showConnectHint = ready && !hasPairing;

  return (
    <Box sx={PAGE_STACK_OUTER_SX}>
      <AccountDetailDialog
        open={dialog.kind !== "closed"}
        mode={dialog.kind === "create" ? "create" : "detail"}
        accountKey={dialog.kind === "detail" ? dialog.accountKey : null}
        providerFilter={capFilter}
        onClose={() => setDialog({ kind: "closed" })}
        onDeleted={load}
        onCreated={(key) => {
          void load();
          setDialog({ kind: "detail", accountKey: key });
        }}
      />
      <InlineAlert message={error || null} onRetry={load} />
      <SettingsSection
        pinHeader
        surfaceTone={loading ? "loading" : "default"}
        sx={{ flex: 1, minHeight: 0 }}
        icon={<Os3dIcon src={OS_ICON_NAV["/accounts"]} />}
        label={t("accounts.sectionTitle")}
        description={t("accounts.sectionDesc")}
        accessory={
          <Button
            variant="contained"
            size="small"
            startIcon={<AddRounded />}
            disabled={!hasPairing || showConnectHint || unsupportedEndpoint}
            onClick={() => setDialog({ kind: "create" })}
          >
            {t("accounts.addAccount")}
          </Button>
        }
      >
        {showConnectHint ? (
          <PanelStateBlock
            tone="neutral"
            icon={<Os3dIcon src={OS_ICON_NAV["/accounts"]} variant="inline" />}
            title={t("accounts.needPairingTitle")}
            description={t("accounts.needPairingDesc")}
            size="compact"
          />
        ) : null}
        {loading ? (
          <PanelStateLoading>
            <SectionLoadingSkeleton />
          </PanelStateLoading>
        ) : unsupportedEndpoint ? (
          <PanelStateBlock
            tone="neutral"
            icon={<Os3dIcon src={OS_ICON_NAV["/accounts"]} variant="inline" />}
            title={t("accounts.unsupportedTitle")}
            description={t("accounts.unsupportedDesc")}
          />
        ) : (
          <Stack spacing={2} sx={{ width: "100%" }}>
            <Box
              sx={{
                display: "flex",
                flexWrap: "wrap",
                alignItems: "center",
                gap: 1.5,
              }}
            >
              <ToggleButtonGroup
                exclusive
                size="small"
                value={capFilter}
                onChange={(_, v: CapabilityFilter | null) => {
                  if (v != null) setCapFilter(v);
                }}
                aria-label={t("accounts.filterByCapability")}
                sx={ACCOUNT_FILTER_GROUP_SX}
              >
                <ToggleButton value="all">{t("accounts.filterAll")}</ToggleButton>
                {CAPABILITIES.map((c) => (
                  <ToggleButton key={c} value={c}>
                    {t(`accounts.capability.${c}`)}
                  </ToggleButton>
                ))}
              </ToggleButtonGroup>
            </Box>

            {!loading && items.length === 0 && !showConnectHint ? (
              <PanelStateBlock
                tone="neutral"
                icon={
                  <Os3dIcon src={OS_ICON_NAV["/accounts"]} variant="inline" />
                }
                title={t("accounts.emptyTitle")}
                description={t("accounts.emptyDesc")}
              />
            ) : null}

            <Stack spacing={1.5} sx={{ width: "100%" }}>
              {items.map((row) => (
                <Box
                  key={row.account_key}
                  component="button"
                  type="button"
                  onClick={() =>
                    setDialog({ kind: "detail", accountKey: row.account_key })
                  }
                  sx={{
                    ...CONFIG_PANEL_SX,
                    cursor: "pointer",
                    textAlign: "left",
                    width: "100%",
                    p: 2,
                    display: "flex",
                    alignItems: "flex-start",
                    justifyContent: "space-between",
                    gap: 2,
                    border: "none",
                    font: "inherit",
                    color: "inherit",
                    transition:
                      "box-shadow var(--transition-duration) var(--ease-out-smooth), transform var(--transition-duration) var(--ease-out-smooth)",
                    "@media (hover: hover)": {
                      "&:hover": {
                        transform: "translateY(-1px)",
                      },
                    },
                  }}
                >
                  <Box sx={{ minWidth: 0, flex: 1 }}>
                    <Typography
                      variant="subtitle1"
                      fontWeight={700}
                      sx={{ lineHeight: 1.3 }}
                    >
                      {row.account_label || row.account_key}
                    </Typography>
                    <Typography variant="body2" color="text.secondary" sx={{ mt: 0.25 }}>
                      {localizeAccountProviderName(t, row.provider_kind)}{" "}
                      · {row.provider_kind}
                    </Typography>
                    <Stack
                      direction="row"
                      flexWrap="wrap"
                      gap={0.75}
                      sx={{ mt: 1.25 }}
                    >
                      <Chip
                        size="small"
                        label={t(`accounts.identity.${row.identity_class}`)}
                        variant="outlined"
                      />
                      {row.enabled_capabilities.map((c) => (
                        <Chip
                          key={c}
                          size="small"
                          label={t(`accounts.capabilityShort.${c}`)}
                        />
                      ))}
                      <Chip
                        size="small"
                        color={readinessColor(row.readiness)}
                        label={t(`accounts.readiness.${row.readiness}`)}
                      />
                      {row.has_runtime_error ? (
                        <Chip
                          size="small"
                          color="error"
                          label={t("accounts.runtimeErrorFlag")}
                        />
                      ) : null}
                    </Stack>
                  </Box>
                  <ChevronRightRounded
                    sx={{ color: "var(--text-tertiary)", flexShrink: 0, mt: 0.25 }}
                    aria-hidden
                  />
                </Box>
              ))}
            </Stack>
          </Stack>
        )}
      </SettingsSection>
    </Box>
  );
}
