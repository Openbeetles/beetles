import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { alpha } from "@mui/material/styles";
import Box from "@mui/material/Box";
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
  buildAccountCardModel,
  type AccountCardStatusColor,
} from "./accountsCardModel";
import {
  CONFIG_PANEL_SX,
  PAGE_STACK_OUTER_SX,
} from "../theme/panelStyles";
import { LIST_CARD_PLATE_BACKGROUND_IMAGE } from "../theme/listItemStyles";

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

/** 就绪状态圆点：与 MUI Chip semantic 对齐 */
const READINESS_DOT_BG: Record<AccountCardStatusColor, string> = {
  success: "success.main",
  warning: "warning.main",
  error: "error.main",
  default: "text.disabled",
};

const ACCOUNT_CARD_STATUS_PILL_SX = {
  display: "inline-flex",
  alignItems: "center",
  gap: 0.65,
  maxWidth: "100%",
  px: 0.85,
  py: 0.4,
  borderRadius: LAYOUT_TOKENS.radiusSearchPill,
  bgcolor: "color-mix(in srgb, var(--foreground) 3.2%, transparent)",
  border: "1px solid color-mix(in srgb, var(--foreground) 5%, transparent)",
} as const;

const ACCOUNT_CARD_SX = {
  ...CONFIG_PANEL_SX,
  cursor: "pointer",
  textAlign: "left",
  width: "100%",
  minWidth: 0,
  overflow: "hidden",
  p: { xs: 2, sm: 2.5 },
  display: "flex",
  flexDirection: "column",
  alignItems: "stretch",
  gap: 1.5,
  font: "inherit",
  color: "inherit",
  bgcolor: "var(--card)",
  backgroundImage: LIST_CARD_PLATE_BACKGROUND_IMAGE,
  boxShadow: [
    "var(--os3d-content-plate-stack)",
    "inset 0 1px 0 color-mix(in srgb, #fff 48%, transparent)",
  ].join(", "),
  transition: [
    "transform var(--transition-duration) var(--ease-out-smooth)",
    "box-shadow var(--transition-duration) var(--ease-out-smooth)",
  ].join(", "),
  "&:focus-visible": {
    outline: "2px solid color-mix(in srgb, var(--primary) 55%, transparent)",
    outlineOffset: 2,
  },
  "@media (hover: hover)": {
    "&:hover": {
      transform: "translateY(-1px)",
      boxShadow: [
        "var(--os3d-content-plate-stack)",
        "0 14px 36px -26px color-mix(in srgb, var(--foreground) 10%, transparent)",
        "inset 0 1px 0 color-mix(in srgb, #fff 52%, transparent)",
      ].join(", "),
      "& [data-account-card-cta]": {
        color: "var(--primary)",
        opacity: 1,
      },
      "& [data-account-card-cta] .MuiSvgIcon-root": {
        transform: "translateX(3px)",
        opacity: 1,
      },
    },
  },
} as const;

type AccountsDialogState =
  | { kind: "closed" }
  | { kind: "create" }
  | { kind: "detail"; accountKey: string };

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

            <Box
              sx={{
                width: "100%",
                display: "grid",
                gridTemplateColumns: {
                  xs: "1fr",
                  md: "repeat(auto-fit, minmax(440px, 520px))",
                },
                gridAutoRows: "1fr",
                gap: 2,
                justifyContent: "start",
              }}
            >
              {items.map((row) => {
                const model = buildAccountCardModel(row, {
                  t,
                  providerLabel: localizeAccountProviderName(t, row.provider_kind),
                });

                return (
                  <Box
                    key={row.account_key}
                    component="button"
                    type="button"
                    onClick={() =>
                      setDialog({ kind: "detail", accountKey: row.account_key })
                    }
                    sx={ACCOUNT_CARD_SX}
                  >
                    <Box
                      sx={{
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "space-between",
                        gap: 1.5,
                        minWidth: 0,
                      }}
                    >
                      <Typography
                        variant="caption"
                        sx={{
                          fontWeight: 600,
                          letterSpacing: "0.05em",
                          color: "var(--text-secondary)",
                          textTransform: "uppercase",
                          fontSize: "0.6875rem",
                          minWidth: 0,
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                          flex: "1 1 auto",
                          opacity: 0.92,
                        }}
                        title={model.eyebrow}
                      >
                        {model.eyebrow}
                      </Typography>
                      <Stack
                        direction="row"
                        flexWrap="wrap"
                        spacing={0}
                        sx={{
                          gap: 0.75,
                          justifyContent: "flex-end",
                          flex: "0 1 auto",
                          maxWidth: { xs: "100%", sm: "min(100%, 320px)" },
                        }}
                      >
                        <Box sx={ACCOUNT_CARD_STATUS_PILL_SX}>
                          <Box
                            sx={{
                              width: 8,
                              height: 8,
                              borderRadius: "50%",
                              flexShrink: 0,
                              bgcolor: READINESS_DOT_BG[model.readinessColor],
                              boxShadow: [
                                "inset 0 1px 0 color-mix(in srgb, #fff 35%, transparent)",
                                "0 0 0 1px color-mix(in srgb, var(--foreground) 7%, transparent)",
                              ].join(", "),
                            }}
                          />
                          <Typography
                            variant="caption"
                            sx={{
                              fontWeight: 600,
                              color: "var(--text-secondary)",
                              lineHeight: 1.25,
                              whiteSpace: "nowrap",
                              overflow: "hidden",
                              textOverflow: "ellipsis",
                              maxWidth: { xs: 132, sm: 192 },
                              fontSize: "0.75rem",
                            }}
                            title={model.readinessLabel}
                          >
                            {model.readinessLabel}
                          </Typography>
                        </Box>
                        {model.showRuntimeFlag ? (
                          <Box
                            sx={(theme) => ({
                              ...ACCOUNT_CARD_STATUS_PILL_SX,
                              bgcolor: alpha(
                                theme.palette.error.main,
                                theme.palette.mode === "dark" ? 0.14 : 0.07,
                              ),
                              border: `1px solid ${alpha(theme.palette.error.main, 0.24)}`,
                            })}
                          >
                            <Box
                              sx={(theme) => ({
                                width: 8,
                                height: 8,
                                borderRadius: "50%",
                                flexShrink: 0,
                                bgcolor: "error.main",
                                boxShadow: [
                                  "inset 0 1px 0 color-mix(in srgb, #fff 28%, transparent)",
                                  `0 0 0 1px ${alpha(theme.palette.error.main, 0.38)}`,
                                ].join(", "),
                              })}
                            />
                            <Typography
                              variant="caption"
                              sx={{
                                fontWeight: 600,
                                color: "error.main",
                                lineHeight: 1.25,
                                whiteSpace: "nowrap",
                                fontSize: "0.75rem",
                              }}
                            >
                              {t("accounts.runtimeErrorFlag")}
                            </Typography>
                          </Box>
                        ) : null}
                      </Stack>
                    </Box>

                    <Typography
                      component="div"
                      sx={{
                        fontSize: "1.1875rem",
                        fontWeight: 600,
                        lineHeight: 1.35,
                        letterSpacing: "-0.015em",
                        color: "var(--foreground)",
                        display: "-webkit-box",
                        WebkitBoxOrient: "vertical",
                        WebkitLineClamp: 2,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        textAlign: "left",
                        mt: -0.25,
                      }}
                      title={model.title}
                    >
                      {model.title}
                    </Typography>

                    <Box
                      sx={{
                        display: "flex",
                        alignItems: "center",
                        gap: 1,
                        minWidth: 0,
                        mt: -0.15,
                      }}
                    >
                      <Box
                        component="span"
                        sx={{
                          px: 0.9,
                          py: 0.25,
                          borderRadius: "var(--radius-chip)",
                          flexShrink: 0,
                          border:
                            "1px solid color-mix(in srgb, var(--primary) 16%, transparent)",
                          bgcolor:
                            "color-mix(in srgb, var(--primary) 5.5%, transparent)",
                          color: "var(--text-secondary)",
                          fontWeight: 600,
                          fontSize: "0.8125rem",
                          lineHeight: 1.45,
                        }}
                      >
                        {model.identityLabel}
                      </Box>
                      <Typography
                        variant="body2"
                        component="span"
                        sx={{
                          color: "var(--text-secondary)",
                          fontWeight: 500,
                          lineHeight: 1.45,
                          minWidth: 0,
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                        }}
                        title={model.capabilityLabels.join(" · ")}
                      >
                        {model.capabilityLabels.join(" · ")}
                      </Typography>
                    </Box>

                    <Box
                      sx={{
                        mt: "auto",
                        pt: 1.25,
                        borderTop:
                          "1px solid color-mix(in srgb, var(--border) 38%, transparent)",
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "space-between",
                        gap: 1.5,
                        minWidth: 0,
                      }}
                    >
                      <Stack spacing={0.2} sx={{ minWidth: 0, flex: "1 1 auto" }}>
                        <Typography
                          variant="caption"
                          component="span"
                          sx={{
                            fontFamily: "var(--font-mono)",
                            fontSize: "0.65625rem",
                            lineHeight: 1.4,
                            color: "var(--text-tertiary)",
                            fontWeight: 500,
                            letterSpacing: "0.02em",
                            display: "block",
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                          title={model.providerMeta}
                        >
                          {model.providerMeta}
                        </Typography>
                        {model.rawKeyMeta ? (
                          <Typography
                            variant="caption"
                            component="span"
                            sx={{
                              fontFamily: "var(--font-mono)",
                              fontSize: "0.625rem",
                              lineHeight: 1.4,
                              color: "color-mix(in srgb, var(--text-tertiary) 75%, transparent)",
                              fontWeight: 400,
                              display: "block",
                              overflow: "hidden",
                              textOverflow: "ellipsis",
                              whiteSpace: "nowrap",
                            }}
                            title={model.rawKeyMeta}
                          >
                            {model.rawKeyMeta}
                          </Typography>
                        ) : null}
                      </Stack>
                      <Box
                        data-account-card-cta
                        sx={{
                          display: "inline-flex",
                          alignItems: "center",
                          gap: 0.2,
                          flexShrink: 0,
                          color:
                            "color-mix(in srgb, var(--primary) 88%, var(--foreground))",
                          opacity: 0.9,
                          transition: [
                            "color var(--transition-duration) var(--ease-out-smooth)",
                            "opacity var(--transition-duration) var(--ease-out-smooth)",
                          ].join(", "),
                        }}
                      >
                        <Typography
                          variant="caption"
                          sx={{
                            fontWeight: 700,
                            letterSpacing: "0.03em",
                          }}
                        >
                          {t("accounts.openDetail")}
                        </Typography>
                        <ChevronRightRounded
                          sx={{
                            fontSize: 18,
                            opacity: 0.88,
                            transition: [
                              "transform var(--transition-duration) var(--ease-out-smooth)",
                              "opacity var(--transition-duration) var(--ease-out-smooth)",
                            ].join(", "),
                          }}
                          aria-hidden
                        />
                      </Box>
                    </Box>
                  </Box>
                );
              })}
            </Box>
          </Stack>
        )}
      </SettingsSection>
    </Box>
  );
}
