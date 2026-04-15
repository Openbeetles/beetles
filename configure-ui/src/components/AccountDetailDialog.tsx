import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Chip from "@mui/material/Chip";
import Dialog from "@mui/material/Dialog";
import DialogContent from "@mui/material/DialogContent";
import DialogTitle from "@mui/material/DialogTitle";
import IconButton from "@mui/material/IconButton";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import CloseRounded from "@mui/icons-material/CloseRounded";
import {
  PanelStateBlock,
  PanelStateLoading,
  SectionLoadingSkeleton,
} from "./form";
import { ConfirmDialog } from "./ConfirmDialog";
import { Os3dIcon } from "./Os3dIcon";
import { OS_ICON_NAV } from "../config/osIcons";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { localizeAccountProviderName } from "../i18n/providerDisplay";
import type { AccountDetail } from "../types/accountConfig";
import { CONFIG_PANEL_SX, TEXT_BODY_TERTIARY_SX } from "../theme/panelStyles";
import {
  AccountCreateForm,
  type AccountCapabilityFilter,
} from "./AccountCreateForm";

export interface AccountDetailDialogProps {
  open: boolean;
  /** 创建：无需 account_key；详情：需已有账户 key */
  mode: "detail" | "create";
  accountKey: string | null;
  /** 创建账户时用于筛选提供方目录（与列表页能力筛选一致） */
  providerFilter?: AccountCapabilityFilter;
  onClose: () => void;
  /** 删除成功后回调（用于刷新列表） */
  onDeleted?: () => void;
  /** 创建成功后可由父级切换到详情并传入新 key */
  onCreated?: (accountKey: string) => void;
}

export function AccountDetailDialog({
  open,
  mode,
  accountKey,
  providerFilter = "all",
  onClose,
  onDeleted,
  onCreated,
}: AccountDetailDialogProps) {
  const { t } = useTranslation();
  const { api, ready, hasPairing } = useDeviceApi();
  const [detail, setDetail] = useState<AccountDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [probeMsg, setProbeMsg] = useState<string | null>(null);
  const [probeBusy, setProbeBusy] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [deleteBusy, setDeleteBusy] = useState(false);

  const dialogVisible = open && (mode === "create" || accountKey != null);

  const load = useCallback(async () => {
    if (!ready || !accountKey) return;
    setLoading(true);
    setError("");
    const res = await api.config.accounts.get(accountKey);
    if (res.ok && res.data) {
      setDetail(res.data);
    } else {
      setError(res.error ?? t("accounts.detailLoadFailed"));
      setDetail(null);
    }
    setLoading(false);
  }, [accountKey, api.config.accounts, ready, t]);

  useEffect(() => {
    if (!open || mode !== "detail" || !accountKey) {
      if (!open) {
        queueMicrotask(() => {
          setDetail(null);
          setError("");
          setProbeMsg(null);
        });
      }
      return;
    }
    // eslint-disable-next-line react-hooks/set-state-in-effect -- opening detail must immediately kick off account fetch
    void load();
  }, [open, mode, accountKey, load]);

  const handleProbe = async () => {
    if (!hasPairing || !accountKey) {
      setProbeMsg(t("accounts.needPairingTitle"));
      return;
    }
    setProbeBusy(true);
    setProbeMsg(null);
    const res = await api.config.accounts.probe(accountKey);
    setProbeBusy(false);
    if (res.ok && res.data) {
      setProbeMsg(`${res.data.disposition}: ${res.data.reason}`.trim());
      void load();
    } else {
      setProbeMsg(res.error ?? t("accounts.probeFailed"));
    }
  };

  const handleDelete = async () => {
    if (!hasPairing || !accountKey) return;
    setDeleteBusy(true);
    const res = await api.config.accounts.delete(accountKey);
    setDeleteBusy(false);
    setDeleteOpen(false);
    if (res.ok) {
      onDeleted?.();
      onClose();
    } else {
      setError(res.error ?? t("accounts.deleteFailed"));
    }
  };

  const a = detail?.account;
  const asmt = detail?.assessment;
  const titleLabel = a?.account_label ?? accountKey ?? "";

  const titleId =
    mode === "create" ? "account-create-dialog-title" : "account-detail-dialog-title";

  return (
    <>
      <Dialog
        open={dialogVisible}
        onClose={onClose}
        maxWidth="md"
        fullWidth
        scroll="paper"
        aria-labelledby={titleId}
        slotProps={{
          paper: {
            sx: {
              maxHeight: "min(92vh, 900px)",
              display: "flex",
              flexDirection: "column",
            },
          },
        }}
      >
        <DialogTitle
          id={titleId}
          component="div"
          sx={{
            display: "flex",
            alignItems: "flex-start",
            justifyContent: "space-between",
            gap: 1,
            pr: 1,
            flexShrink: 0,
          }}
        >
          <Box sx={{ minWidth: 0 }}>
            <Typography variant="h6" component="span" fontWeight={700}>
              {mode === "create" ? t("accounts.addTitle") : t("accounts.detailTitle")}
            </Typography>
            <Typography
              variant="body2"
              color="text.secondary"
              sx={{
                mt: 0.25,
                ...(mode === "create"
                  ? {}
                  : { overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }),
              }}
              title={
                mode === "create" ? t("accounts.addSubtitle") : titleLabel
              }
            >
              {mode === "create" ? t("accounts.addSubtitle") : titleLabel}
            </Typography>
          </Box>
          <IconButton
            aria-label={t("accounts.closeDetail")}
            onClick={onClose}
            size="small"
            sx={{ flexShrink: 0 }}
          >
            <CloseRounded />
          </IconButton>
        </DialogTitle>
        <DialogContent
          sx={{
            flex: "1 1 auto",
            minHeight: 0,
            pt: 2,
            ...(mode === "create"
              ? {
                  overflow: "hidden",
                  display: "flex",
                  flexDirection: "column",
                }
              : { overflow: "auto" }),
          }}
        >
          {mode === "create" ? (
            <Box
              sx={{
                flex: 1,
                minHeight: 0,
                display: "flex",
                flexDirection: "column",
              }}
            >
              <AccountCreateForm
                capabilityFilter={providerFilter}
                onCreated={(key) => onCreated?.(key)}
              />
            </Box>
          ) : (
            <>
              {error ? (
                <Typography color="error" variant="body2" sx={{ mb: 2 }}>
                  {error}
                </Typography>
              ) : null}

              {loading ? (
                <PanelStateLoading>
                  <SectionLoadingSkeleton />
                </PanelStateLoading>
              ) : !detail || !a ? (
                <PanelStateBlock
                  tone="neutral"
                  icon={
                    <Os3dIcon
                      src={OS_ICON_NAV["/accounts"]}
                      variant="inline"
                    />
                  }
                  title={t("accounts.detailMissingTitle")}
                  description={t("accounts.detailMissingDesc")}
                />
              ) : (
                <Stack spacing={2} sx={{ width: "100%" }}>
                  <Box sx={{ ...CONFIG_PANEL_SX, p: 2 }}>
                    <Typography variant="subtitle2" color="text.secondary">
                      {t("accounts.detailProvider")}
                    </Typography>
                    <Typography fontWeight={700}>
                      {localizeAccountProviderName(t, a.provider_kind)}
                    </Typography>
                    <Typography
                      variant="body2"
                      sx={{ ...TEXT_BODY_TERTIARY_SX, mt: 0.5 }}
                    >
                      {a.provider_kind} · {a.account_key}
                    </Typography>
                    {a.external_account_id ? (
                      <Typography variant="body2" sx={{ mt: 1 }}>
                        {a.external_account_id}
                      </Typography>
                    ) : null}
                  </Box>

                  {asmt ? (
                    <Box sx={{ ...CONFIG_PANEL_SX, p: 2 }}>
                      <Typography
                        variant="subtitle2"
                        color="text.secondary"
                        gutterBottom
                      >
                        {t("accounts.detailAssessment")}
                      </Typography>
                      <Stack direction="row" flexWrap="wrap" gap={1}>
                        <Chip
                          size="small"
                          label={t(`accounts.readiness.${asmt.readiness}`)}
                        />
                        <Chip
                          size="small"
                          variant="outlined"
                          label={t(`accounts.nextAction.${asmt.next_action}`)}
                        />
                      </Stack>
                      {asmt.missing_fields.length > 0 ? (
                        <Typography variant="body2" sx={{ mt: 1.5 }}>
                          {t("accounts.missingFields")}:{" "}
                          {asmt.missing_fields.join(", ")}
                        </Typography>
                      ) : null}
                    </Box>
                  ) : null}

                  {probeMsg ? (
                    <Typography variant="body2" color="text.secondary">
                      {probeMsg}
                    </Typography>
                  ) : null}

                  <Stack direction="row" flexWrap="wrap" gap={1}>
                    <Button
                      variant="contained"
                      disabled={!hasPairing || probeBusy}
                      onClick={() => void handleProbe()}
                    >
                      {probeBusy ? t("accounts.probing") : t("accounts.probe")}
                    </Button>
                    <Button
                      color="error"
                      variant="outlined"
                      disabled={!hasPairing || deleteBusy}
                      onClick={() => setDeleteOpen(true)}
                    >
                      {t("accounts.delete")}
                    </Button>
                  </Stack>

                  <Typography
                    variant="caption"
                    color="text.secondary"
                    sx={{ display: "block" }}
                  >
                    {t("accounts.editHint")}
                  </Typography>
                </Stack>
              )}
            </>
          )}
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={deleteOpen}
        onClose={() => setDeleteOpen(false)}
        title={t("accounts.deleteConfirmTitle")}
        description={t("accounts.deleteConfirmDesc")}
        confirmColor="error"
        confirmLabel={t("accounts.delete")}
        confirmDisabled={deleteBusy}
        onConfirm={() => void handleDelete()}
      />
    </>
  );
}
