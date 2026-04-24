import { useCallback, useEffect, useRef, useState } from "react";
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
  InlineAlert,
  PageLoadErrorState,
  PanelStateBlock,
  PanelStateLoading,
  SaveFeedback,
  SectionLoadingSkeleton,
  splitPageErrorState,
} from "./form";
import { ConfirmDialog } from "./ConfirmDialog";
import { Os3dIcon } from "./Os3dIcon";
import { OS_ICON_NAV } from "../config/osIcons";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { useSaveFeedback } from "../hooks/useSaveFeedback";
import { localizeAccountProviderName } from "../i18n/providerDisplay";
import {
  localizeProviderField,
  localizeProviderFieldLabel,
} from "../i18n/providerFields";
import type { AccountDetail } from "../types/accountConfig";
import {
  CONFIG_PANEL_SX,
  DIALOG_FORM_SCROLL_WELL_SX,
  DIALOG_FORM_SUBMIT_BAR_SX,
  TEXT_BODY_TERTIARY_SX,
} from "../theme/panelStyles";
import {
  AccountCreateForm,
  type AccountCapabilityFilter,
} from "./AccountCreateForm";
import { ProviderFieldInput } from "./ProviderFieldInput";
import { formatProbeMessage } from "./accountDetailDialogHelpers";
import { translateApiError } from "../i18n/apiErrors";
import { errorMessage, withTimeout } from "../util/withTimeout";
import { createLatestRequestGuard } from "../util/latestRequest";

const ACCOUNT_REQUEST_TIMEOUT_MS = 15_000;

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
  const { api, ready, canAccessProtectedApis } = useDeviceApi();
  const [detail, setDetail] = useState<AccountDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [probeMsg, setProbeMsg] = useState<string | null>(null);
  const [probeBusy, setProbeBusy] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [fieldValues, setFieldValues] = useState<Record<string, string>>({});
  const detailLoadGuardRef = useRef(createLatestRequestGuard());
  const probeGuardRef = useRef(createLatestRequestGuard());
  const deleteGuardRef = useRef(createLatestRequestGuard());
  const saveFeedback = useSaveFeedback(t);
  const {
    status: saveStatus,
    error: saveError,
    begin: beginSaveFeedback,
    fail: failSaveFeedback,
    finishFromResult: finishSaveFeedbackFromResult,
    dismiss: dismissSaveFeedback,
  } = saveFeedback;

  const dialogVisible = open && (mode === "create" || accountKey != null);
  const localizedDetailFields = detail?.fields.map((field) =>
    localizeProviderField(t, field),
  ) ?? [];
  const detailErrorState = splitPageErrorState({
    hasData: Boolean(detail),
    loading,
    error,
  });

  const load = useCallback(async () => {
    if (!ready || !accountKey) return;
    const requestId = detailLoadGuardRef.current.next();
    setLoading(true);
    setError("");
    try {
      const res = await withTimeout(
        api.config.accounts.get(accountKey),
        ACCOUNT_REQUEST_TIMEOUT_MS,
        t("accounts.requestTimedOut"),
      );
      if (!detailLoadGuardRef.current.isCurrent(requestId)) return;
      if (res.ok && res.data) {
        setDetail(res.data);
        const nextValues: Record<string, string> = {};
        for (const field of res.data.fields) {
          nextValues[field.key] = field.current_value ?? "";
        }
        setFieldValues(nextValues);
      } else {
        setError(translateApiError(t, res.error, "accounts.detailLoadFailed"));
        setDetail(null);
      }
    } catch (error) {
      if (!detailLoadGuardRef.current.isCurrent(requestId)) return;
      setError(errorMessage(error, t("accounts.detailLoadFailed")));
      setDetail(null);
    }
    if (!detailLoadGuardRef.current.isCurrent(requestId)) return;
    setLoading(false);
  }, [accountKey, api.config.accounts, ready, t]);

  useEffect(() => {
    const detailGuard = detailLoadGuardRef.current;
    const probeGuard = probeGuardRef.current;
    const deleteGuard = deleteGuardRef.current;
    detailGuard.invalidate();
    probeGuard.invalidate();
    deleteGuard.invalidate();
    let cancelled = false;
    const resetDetailState = () => {
      if (cancelled) return;
      setDetail(null);
      setError("");
      setProbeMsg(null);
      setLoading(false);
      setProbeBusy(false);
      setDeleteOpen(false);
      setDeleteBusy(false);
      setFieldValues({});
      dismissSaveFeedback();
    };
    if (!open || mode !== "detail" || !accountKey) {
      if (!open) {
        queueMicrotask(resetDetailState);
      }
      return () => {
        cancelled = true;
      };
    }
    queueMicrotask(() => {
      resetDetailState();
      if (!cancelled) void load();
    });
    return () => {
      cancelled = true;
      detailGuard.invalidate();
      probeGuard.invalidate();
      deleteGuard.invalidate();
    };
  }, [open, mode, accountKey, load, dismissSaveFeedback]);

  const handleProbe = async () => {
    if (!canAccessProtectedApis || !accountKey) {
      setProbeMsg(t("accounts.needPairingTitle"));
      return;
    }
    const requestId = probeGuardRef.current.next();
    setProbeBusy(true);
    setProbeMsg(null);
    try {
      const res = await withTimeout(
        api.config.accounts.probe(accountKey),
        ACCOUNT_REQUEST_TIMEOUT_MS,
        t("accounts.requestTimedOut"),
      );
      if (!probeGuardRef.current.isCurrent(requestId)) return;
      setProbeBusy(false);
      if (res.ok && res.data) {
        setProbeMsg(formatProbeMessage(res.data.disposition, res.data.reason, t));
        void load();
      } else {
        setProbeMsg(translateApiError(t, res.error, "accounts.probeFailed"));
      }
    } catch (error) {
      if (!probeGuardRef.current.isCurrent(requestId)) return;
      setProbeBusy(false);
      setProbeMsg(errorMessage(error, t("accounts.probeFailed")));
    }
  };

  const handleDelete = async () => {
    if (!canAccessProtectedApis || !accountKey) return;
    const requestId = deleteGuardRef.current.next();
    setDeleteBusy(true);
    try {
      const res = await withTimeout(
        api.config.accounts.delete(accountKey),
        ACCOUNT_REQUEST_TIMEOUT_MS,
        t("accounts.requestTimedOut"),
      );
      if (!deleteGuardRef.current.isCurrent(requestId)) return;
      setDeleteBusy(false);
      setDeleteOpen(false);
      if (res.ok) {
        onDeleted?.();
        onClose();
      } else {
        setError(translateApiError(t, res.error, "accounts.deleteFailed"));
      }
    } catch (error) {
      if (!deleteGuardRef.current.isCurrent(requestId)) return;
      setDeleteBusy(false);
      setDeleteOpen(false);
      setError(errorMessage(error, t("accounts.deleteFailed")));
    }
  };

  const handleSaveConfig = async () => {
    if (!canAccessProtectedApis || !accountKey || !detail) return;
    const invalid = localizedDetailFields.filter(
      (field) =>
        field.required &&
        !(fieldValues[field.key] ?? "").trim() &&
        !(field.secret && field.configured),
    );
    if (invalid.length > 0) {
      setError(
        t("accounts.createRequiredFields", {
          fields: invalid
            .map((field) => localizeProviderFieldLabel(t, field))
            .join(", "),
        }),
      );
      return;
    }
    const fields: Record<string, string> = {};
    const clear_fields: string[] = [];
    for (const field of detail.fields) {
      const raw = (fieldValues[field.key] ?? "").trim();
      if (raw) {
        fields[field.key] = raw;
        continue;
      }
      if (field.secret) {
        continue;
      }
      if (field.configured && (field.current_value ?? "").trim()) {
        clear_fields.push(field.key);
      }
    }

    beginSaveFeedback();
    setError("");
    try {
      const res = await withTimeout(
        api.config.accounts.saveConfig(accountKey, {
          fields,
          clear_fields,
        }),
        ACCOUNT_REQUEST_TIMEOUT_MS,
        t("accounts.requestTimedOut"),
      );
      finishSaveFeedbackFromResult(res);
      if (res.ok && res.data) {
        setDetail(res.data);
        const nextValues: Record<string, string> = {};
        for (const field of res.data.fields) {
          nextValues[field.key] = field.current_value ?? "";
        }
        setFieldValues(nextValues);
        setProbeMsg(null);
      } else if (!res.ok) {
        setError(translateApiError(t, res.error, "accounts.saveConfigFailed"));
      }
    } catch (error) {
      const message = errorMessage(error, t("accounts.saveConfigFailed"));
      failSaveFeedback(message);
      setError(message);
    }
  };

  const a = detail?.account;
  const asmt = detail?.assessment;
  const titleLabel = a?.account_label ?? accountKey ?? "";

  const titleId =
    mode === "create" ? "account-create-dialog-title" : "account-detail-dialog-title";

  const detailFooter = detail && a ? (
    <Box sx={DIALOG_FORM_SUBMIT_BAR_SX}>
      <Stack direction="row" flexWrap="wrap" gap={1} justifyContent="flex-end">
        <Button
          color="error"
          variant="outlined"
          disabled={!canAccessProtectedApis || deleteBusy}
          onClick={() => setDeleteOpen(true)}
          sx={{ minWidth: 112 }}
        >
          {t("accounts.delete")}
        </Button>
        <Button
          variant="contained"
          disabled={!canAccessProtectedApis || saveStatus === "saving"}
          onClick={() => void handleSaveConfig()}
          sx={{ minWidth: 128 }}
        >
          {saveStatus === "saving" ? t("common.saving") : t("common.save")}
        </Button>
      </Stack>
      {saveStatus === "ok" ? (
        <SaveFeedback
          status="ok"
          message={t("common.saveOk")}
          onDismiss={dismissSaveFeedback}
        />
      ) : null}
      {saveStatus === "fail" && saveError ? (
        <SaveFeedback
          status="fail"
          message={saveError}
          onDismiss={dismissSaveFeedback}
        />
      ) : null}
    </Box>
  ) : null;

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
            overflow: "hidden",
            display: "flex",
            flexDirection: "column",
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
            <Stack
              spacing={0}
              sx={{
                width: "100%",
                flex: 1,
                minHeight: 0,
                maxHeight: "100%",
                display: "flex",
                flexDirection: "column",
              }}
            >
              <Stack spacing={3} sx={DIALOG_FORM_SCROLL_WELL_SX}>
              <InlineAlert
                message={detailErrorState.inlineError}
                onRetry={() => void load()}
              />

              {loading ? (
                <PanelStateLoading>
                  <SectionLoadingSkeleton />
                </PanelStateLoading>
              ) : detailErrorState.blockingError ? (
                <PageLoadErrorState
                  message={detailErrorState.blockingError}
                  onRetry={() => void load()}
                />
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
                      {localizeAccountProviderName(t, {
                        providerKind: a.provider_kind,
                        displayNameKey: a.display_name_key,
                      })}
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
                          {asmt.missing_fields
                            .map((fieldKey) => {
                              const matchingField = localizedDetailFields.find(
                                (field) => field.key === fieldKey,
                              );
                              return matchingField
                                ? localizeProviderFieldLabel(t, matchingField)
                                : fieldKey;
                            })
                            .join(", ")}
                        </Typography>
                      ) : null}
                      <Stack
                        direction="row"
                        alignItems="center"
                        flexWrap="wrap"
                        gap={1}
                        sx={{ mt: 1.5 }}
                      >
                        <Button
                          variant="outlined"
                          size="small"
                          disabled={!canAccessProtectedApis || probeBusy}
                          onClick={() => void handleProbe()}
                        >
                          {probeBusy ? t("accounts.probing") : t("accounts.probe")}
                        </Button>
                        {probeMsg ? (
                          <Typography variant="body2" color="text.secondary">
                            {probeMsg}
                          </Typography>
                        ) : null}
                      </Stack>
                    </Box>
                  ) : null}

                  <Box sx={{ ...CONFIG_PANEL_SX, p: 2 }}>
                    <Typography
                      variant="subtitle2"
                      color="text.secondary"
                      gutterBottom
                    >
                      {t("accounts.providerFields")}
                    </Typography>
                    <Stack spacing={3}>
                      {localizedDetailFields.map((field) => (
                        <Box key={field.key}>
                          <ProviderFieldInput
                            field={field}
                            value={fieldValues[field.key] ?? ""}
                            onChange={(nextValue) => {
                              setFieldValues((prev) => ({
                                ...prev,
                                [field.key]: nextValue,
                              }));
                            }}
                          />
                        </Box>
                      ))}
                    </Stack>
                  </Box>

                </Stack>
              )}
              </Stack>
              {detailFooter}
            </Stack>
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
