import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import UsbRounded from "@mui/icons-material/UsbRounded";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import TextField from "@mui/material/TextField";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import { ConfirmDialog } from "./ConfirmDialog";
import { BeetleIcon } from "./BeetleIcon";
import { FirmwareFlashDialog } from "./FirmwareFlashDialog";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { useDevice } from "../hooks/useDevice";
import { useRevealedPassword } from "../hooks/useRevealedPassword";
import { useToast } from "../hooks/useToast";
import { getSystem } from "../api/endpoints/config";
import { getPairingCode } from "../api/endpoints/pairingCode";
import { translateApiError } from "../i18n/apiErrors";
import { LAYOUT_TOKENS } from "../config/themeTokens";
import {
  DEFAULT_DEVICE_BASE_URL,
  deriveDeviceAccessStage,
  deriveDeviceSetupCardModel,
  deriveSuccessfulProbeSessionUpdate,
  normalizeDeviceUrl,
  validatePairingCodeDraft,
} from "../pages/deviceAccessFlow";
import {
  markPairingAuthValid,
  setDeviceProbeState,
  setDeviceSessionState,
} from "../store/deviceStatusStore";
import {
  DASHBOARD_CARD_SURFACE_SX,
  PAGE_SCROLL_CANVAS_SX,
} from "../theme/panelStyles";
import { useUnsaved } from "../hooks/useUnsaved";

const DEVICE_ACCESS_DIRTY_OWNER = "device-access-card";

function SetupConnectionEditor({
  urlValue,
  pairingCodeValue,
  pairingCodeReveal,
  onUrlChange,
  onCodeChange,
  onPrimaryAction,
  primaryLabel,
  primaryDisabled,
  baseUrlLabel,
  baseUrlPlaceholder,
  pairingCodeLabel,
  pairingCodePlaceholder,
  showPairingCodeField,
  inputsDisabled,
}: {
  urlValue: string;
  pairingCodeValue: string;
  pairingCodeReveal: ReturnType<typeof useRevealedPassword>;
  onUrlChange: (value: string) => void;
  onCodeChange: (value: string) => void;
  onPrimaryAction: () => void;
  primaryLabel: string;
  primaryDisabled: boolean;
  baseUrlLabel: string;
  baseUrlPlaceholder: string;
  pairingCodeLabel: string;
  pairingCodePlaceholder: string;
  showPairingCodeField: boolean;
  inputsDisabled: boolean;
}) {
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
        disabled={inputsDisabled}
        variant="outlined"
        fullWidth
        slotProps={{
          htmlInput: { style: { fontFamily: "var(--font-mono)" } },
        }}
      />
      {showPairingCodeField ? (
        <TextField
          label={pairingCodeLabel}
          placeholder={pairingCodePlaceholder}
          value={pairingCodeValue}
          type={pairingCodeReveal.type}
          onChange={(e) => onCodeChange(e.target.value)}
          disabled={inputsDisabled}
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
      <Button
        variant="contained"
        onClick={onPrimaryAction}
        disabled={primaryDisabled}
        size="large"
        sx={{
          borderRadius: "var(--radius-full)",
          py: 1.25,
          fontWeight: 600,
          boxShadow: "none",
          "&:hover": { boxShadow: "none" },
        }}
      >
        {primaryLabel}
      </Button>
    </Box>
  );
}

export function DeviceAccessCard() {
  const { t } = useTranslation();
  const { setDirtyFor, clearDirtyFor } = useUnsaved();
  const { baseUrl, pairingCode, setBaseUrl, setPairingCode } = useDevice();
  const { api, appMode } = useDeviceApi();
  const { showToast } = useToast();
  const pairingCodeReveal = useRevealedPassword();
  const [urlInput, setUrlInput] = useState(baseUrl || DEFAULT_DEVICE_BASE_URL);
  const [codeInput, setCodeInput] = useState(pairingCode);
  const [pairingInitCodeInput, setPairingInitCodeInput] = useState("");
  const [pairingInitConfirmOpen, setPairingInitConfirmOpen] = useState(false);
  const [setupDetectedPairingState, setSetupDetectedPairingState] = useState<
    "unknown" | "uninitialized" | "initialized"
  >("unknown");
  const [probeStatus, setProbeStatus] = useState<
    "idle" | "checking" | "ok" | "fail"
  >("idle");
  const [pairingSubmitting, setPairingSubmitting] = useState<
    null | "init_pairing" | "unlock"
  >(null);
  const [flashDialogOpen, setFlashDialogOpen] = useState(false);
  const accessRequestVersionRef = useRef(0);

  const beginAccessRequest = useCallback(() => {
    accessRequestVersionRef.current += 1;
    return accessRequestVersionRef.current;
  }, []);

  const isCurrentAccessRequest = useCallback((requestVersion: number) => {
    return accessRequestVersionRef.current === requestVersion;
  }, []);

  const deviceSessionKey = `${baseUrl ?? ""}\0${pairingCode ?? ""}`;
  const prevDeviceSessionKeyRef = useRef(deviceSessionKey);
  useEffect(() => {
    if (deviceSessionKey === prevDeviceSessionKeyRef.current) return;
    prevDeviceSessionKeyRef.current = deviceSessionKey;
    accessRequestVersionRef.current += 1;
    const nextUrl = baseUrl || DEFAULT_DEVICE_BASE_URL;
    const nextCode = pairingCode ?? "";
    queueMicrotask(() => {
      setUrlInput(nextUrl);
      setCodeInput(nextCode);
      setPairingInitCodeInput("");
      setPairingInitConfirmOpen(false);
      setSetupDetectedPairingState("unknown");
      setProbeStatus("idle");
      setPairingSubmitting(null);
    });
  }, [deviceSessionKey, baseUrl, pairingCode]);

  useEffect(() => {
    return () => {
      accessRequestVersionRef.current += 1;
    };
  }, []);

  const accessStage = deriveDeviceAccessStage(appMode);
  const targetDirty = useMemo(() => {
    return normalizeDeviceUrl(urlInput) !== normalizeDeviceUrl(baseUrl ?? "");
  }, [baseUrl, urlInput]);
  const setupDisplayMode = useMemo(() => {
    if (targetDirty || appMode !== "probing") return appMode;
    if (setupDetectedPairingState === "initialized") return "unlock";
    if (setupDetectedPairingState === "uninitialized") return "init_pairing";
    return appMode;
  }, [appMode, setupDetectedPairingState, targetDirty]);
  const setupCardModel = useMemo(
    () => deriveDeviceSetupCardModel(setupDisplayMode, { targetDirty }),
    [setupDisplayMode, targetDirty],
  );
  const setupPairingCodeValue =
    setupCardModel.stage === "init_pairing" ? pairingInitCodeInput : codeInput;

  const connectionDraftDirty = useMemo(() => {
    const draftUrl = normalizeDeviceUrl(urlInput);
    const savedUrl = normalizeDeviceUrl(baseUrl ?? "");
    const draftCode = (codeInput ?? "").trim();
    const savedCode = (pairingCode ?? "").trim();
    return draftUrl !== savedUrl || draftCode !== savedCode;
  }, [urlInput, codeInput, baseUrl, pairingCode]);
  const pairingInitDirty = useMemo(
    () => pairingInitCodeInput.trim().length > 0,
    [pairingInitCodeInput],
  );

  useEffect(() => {
    setDirtyFor(
      DEVICE_ACCESS_DIRTY_OWNER,
      connectionDraftDirty || pairingInitDirty,
    );
  }, [connectionDraftDirty, pairingInitDirty, setDirtyFor]);

  useEffect(() => {
    return () => clearDirtyFor(DEVICE_ACCESS_DIRTY_OWNER);
  }, [clearDirtyFor]);

  const handleSetupProbe = useCallback(async () => {
    const url = normalizeDeviceUrl(urlInput);
    const requestVersion = beginAccessRequest();
    setProbeStatus("checking");
    const res = await getPairingCode(url);
    if (!isCurrentAccessRequest(requestVersion)) return;
    if (!res.ok || !res.data) {
      setProbeStatus("fail");
      setSetupDetectedPairingState("unknown");
      showToast(
        `${t("device.probeFail")}: ${translateApiError(t, res.error, "common.error")}`,
        {
          variant: "error",
        },
      );
      return;
    }

    const sessionUpdate = deriveSuccessfulProbeSessionUpdate({
      probedUrl: url,
      currentBaseUrl: baseUrl,
      currentPairingCode: pairingCode,
      codeSet: res.data.code_set,
    });
    setProbeStatus("ok");
    setSetupDetectedPairingState(sessionUpdate.detectedPairingState);
    setBaseUrl(sessionUpdate.nextBaseUrl);
    if (sessionUpdate.shouldClearStoredPairing) {
      setPairingCode("");
      setCodeInput("");
    } else {
      setCodeInput((pairingCode ?? "").trim());
    }
    setPairingInitCodeInput("");
    setPairingInitConfirmOpen(false);
    setDeviceSessionState({
      hasTarget: true,
      localPairing: sessionUpdate.nextLocalPairing,
      preserveAuth: sessionUpdate.preserveAuth,
    });
    setDeviceProbeState({
      transport: "reachable",
      devicePairing: sessionUpdate.detectedPairingState,
    });
    showToast(t("device.probeOk"), { variant: "success" });
  }, [
    baseUrl,
    beginAccessRequest,
    isCurrentAccessRequest,
    pairingCode,
    setBaseUrl,
    setPairingCode,
    showToast,
    t,
    urlInput,
  ]);

  const validateProtectedAccess = useCallback(
    async (url: string, candidatePairingCode: string) => {
      const result = await getSystem(url, candidatePairingCode);
      if (result.ok) return null;
      return translateApiError(t, result.error, "device.pairingValidateFailed");
    },
    [t],
  );

  const handleRequestPairingInitialization = useCallback(() => {
    const validationKey = validatePairingCodeDraft(pairingInitCodeInput);
    if (validationKey) {
      showToast(t(validationKey), { variant: "error" });
      return;
    }
    setPairingInitConfirmOpen(true);
  }, [pairingInitCodeInput, showToast, t]);

  const commitValidatedPairing = useCallback(
    (normalizedCode: string) => {
      // Unlock validates a transient candidate code before it is persisted;
      // promote the current session immediately so the shell can leave the access card on the first submit.
      markPairingAuthValid();
      setDeviceSessionState({
        hasTarget: true,
        localPairing: "present",
        preserveAuth: true,
      });
      setPairingCode(normalizedCode);
      setCodeInput(normalizedCode);
    },
    [setPairingCode],
  );

  const handleInitializePairing = useCallback(async () => {
    const requestVersion = beginAccessRequest();
    setPairingSubmitting("init_pairing");
    const normalizedCode = pairingInitCodeInput.trim();
    const initResult = await api.pairing.initialize(normalizedCode);
    if (!isCurrentAccessRequest(requestVersion)) return;
    if (!initResult.ok) {
      if (initResult.error === "pairing.code_already_set") {
        setDeviceProbeState({
          transport: "reachable",
          devicePairing: "initialized",
        });
      }
      showToast(translateApiError(t, initResult.error, "common.error"), {
        variant: "error",
      });
      setPairingSubmitting(null);
      return;
    }
    const url = normalizeDeviceUrl(baseUrl ?? urlInput);
    setDeviceProbeState({
      transport: "reachable",
      devicePairing: "initialized",
    });
    setCodeInput(normalizedCode);
    const validationError = await validateProtectedAccess(url, normalizedCode);
    if (!isCurrentAccessRequest(requestVersion)) return;
    if (validationError) {
      showToast(validationError, { variant: "error" });
      setPairingSubmitting(null);
      return;
    }
    commitValidatedPairing(normalizedCode);
    setPairingInitCodeInput("");
    showToast(t("device.pairingInitSuccess"), { variant: "success" });
    setPairingSubmitting(null);
  }, [
    api.pairing,
    baseUrl,
    beginAccessRequest,
    commitValidatedPairing,
    isCurrentAccessRequest,
    pairingInitCodeInput,
    showToast,
    t,
    urlInput,
    validateProtectedAccess,
  ]);

  const handleUnlock = useCallback(async () => {
    const validationKey = validatePairingCodeDraft(codeInput);
    if (validationKey) {
      showToast(t(validationKey), { variant: "error" });
      return;
    }
    const requestVersion = beginAccessRequest();
    setPairingSubmitting("unlock");
    const normalizedCode = codeInput.trim();
    const url = normalizeDeviceUrl(baseUrl ?? urlInput);
    const validationError = await validateProtectedAccess(url, normalizedCode);
    if (!isCurrentAccessRequest(requestVersion)) return;
    if (validationError) {
      showToast(validationError, { variant: "error" });
      setPairingSubmitting(null);
      return;
    }
    commitValidatedPairing(normalizedCode);
    showToast(t("device.unlockSuccess"), { variant: "success" });
    setPairingSubmitting(null);
  }, [
    baseUrl,
    beginAccessRequest,
    commitValidatedPairing,
    codeInput,
    isCurrentAccessRequest,
    showToast,
    t,
    urlInput,
    validateProtectedAccess,
  ]);

  if (accessStage === "ready") return null;

  const submitting =
    pairingSubmitting === "init_pairing" || pairingSubmitting === "unlock";
  const inputsDisabled = probeStatus === "checking" || submitting;
  const primaryLabel =
    submitting
      ? t("common.saving")
      : probeStatus === "checking"
        ? t("device.probing")
        : setupCardModel.primaryAction === "probe"
          ? t("device.probe")
          : setupCardModel.primaryAction === "save_new_pairing"
            ? t("device.pairingInitSubmit")
            : t("device.save");

  return (
    <>
      <Box
        data-app-scroll-region
        sx={{
          ...PAGE_SCROLL_CANVAS_SX,
          width: "100%",
          minHeight: "100%",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          px: { xs: 0, sm: 1 },
          py: { xs: 2, sm: 2.5, lg: 3 },
          boxSizing: "border-box",
        }}
      >
        <Box
          sx={{
            width: "100%",
            maxWidth: 480,
            mx: "auto",
            ...DASHBOARD_CARD_SURFACE_SX,
            p: { xs: 3, md: 5 },
            position: "relative",
            display: "flex",
            flexDirection: "column",
            gap: 4,
          }}
        >
          <Tooltip title={t("device.flashTriggerHint")} arrow>
            <Button
              onClick={() => setFlashDialogOpen(true)}
              size="small"
              variant="text"
              aria-label={t("device.flashTriggerLabel")}
              sx={{
                position: "absolute",
                right: -4,
                top: -4,
                width: 62,
                height: 62,
                minWidth: 62,
                borderRadius: 0,
                px: 0,
                py: 0,
                border: "none",
                color: "var(--foreground)",
                zIndex: 2,
                overflow: "visible",
                display: "flex",
                alignItems: "flex-start",
                justifyContent: "flex-end",
                "&:hover": {
                  backgroundColor: "transparent",
                  boxShadow: "none",
                },
                "&:before, &:after": {
                  content: '""',
                  position: "absolute",
                  right: 0,
                  top: 0,
                  width: 62,
                  height: 62,
                  pointerEvents: "none",
                },
                "&:before": {
                  backgroundColor: "color-mix(in srgb, var(--surface) 84%, var(--card) 16%)",
                  clipPath: "polygon(100% 0, 0 0, 100% 100%)",
                  boxShadow:
                    "inset 0 1px 0 color-mix(in srgb, var(--border) 42%, transparent)",
                },
                "&:after": {
                  right: 8,
                  top: 8,
                  width: 48,
                  height: 48,
                  backgroundColor: "color-mix(in srgb, var(--card) 88%, var(--primary) 12%)",
                  clipPath: "polygon(100% 0, 0 0, 100% 100%)",
                  filter: "brightness(1.06)",
                },
              }}
            >
              <UsbRounded
                sx={{
                  position: "absolute",
                  right: 10,
                  top: 10,
                  width: 24,
                  height: 24,
                  transform: "rotate(18deg)",
                  filter:
                    "drop-shadow(0 4px 8px color-mix(in srgb, var(--foreground) 16%, transparent))",
                  pointerEvents: "none",
                }}
              />
            </Button>
          </Tooltip>

          <Box sx={{ position: "relative", zIndex: 1, textAlign: "center" }}>
            <Box
              sx={{
                width: "88px",
                height: "88px",
                mx: "auto",
                mb: 3,
                borderRadius: "var(--radius-card)",
                bgcolor: "color-mix(in srgb, var(--primary) 10%, transparent)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                lineHeight: 0,
                color: "var(--primary)",
                boxShadow: [
                  "var(--os3d-pedestal-lift-stack)",
                  "0 14px 28px color-mix(in srgb, var(--primary) 10%, transparent)",
                ].join(", "),
              }}
            >
              <BeetleIcon
                motion="idle"
                sx={{
                  width: "72px",
                  height: "72px",
                  flexShrink: 0,
                  display: "block",
                }}
              />
            </Box>
            <Typography
              variant="h4"
              sx={{
                fontFamily: "var(--font-brand)",
                fontWeight: 800,
                letterSpacing: "-0.02em",
                mb: 1.5,
              }}
            >
              {(() => {
                const full = t("app.name");
                const head = full.replace(/\s*OS\s*$/i, "").trim();
                return (
                  <>
                    {head}{" "}
                    <Box component="span" sx={{ color: "var(--primary)" }}>
                      OS
                    </Box>
                  </>
                );
              })()}
            </Typography>
          </Box>

          <SetupConnectionEditor
            urlValue={urlInput}
            pairingCodeValue={setupPairingCodeValue}
            pairingCodeReveal={pairingCodeReveal}
            onUrlChange={setUrlInput}
            onCodeChange={
              setupCardModel.stage === "init_pairing"
                ? setPairingInitCodeInput
                : setCodeInput
            }
            onPrimaryAction={() => {
              if (setupCardModel.primaryAction === "probe") {
                void handleSetupProbe();
                return;
              }
              if (setupCardModel.primaryAction === "save_new_pairing") {
                handleRequestPairingInitialization();
                return;
              }
              void handleUnlock();
            }}
            primaryLabel={primaryLabel}
            primaryDisabled={probeStatus === "checking" || submitting}
            baseUrlLabel={t("device.baseUrlLabel")}
            baseUrlPlaceholder={t("device.baseUrlPlaceholder")}
            pairingCodeLabel={t("device.pairingCodeLabel")}
            pairingCodePlaceholder={t("device.pairingCodePlaceholder")}
            showPairingCodeField={setupCardModel.showPairingCodeField}
            inputsDisabled={inputsDisabled}
          />
        </Box>
      </Box>
      <ConfirmDialog
        open={pairingInitConfirmOpen}
        onClose={() => setPairingInitConfirmOpen(false)}
        title={t("device.pairingInitConfirmTitle")}
        description={t("device.pairingInitConfirmDesc")}
        confirmLabel={t("device.pairingInitSubmit")}
        onConfirm={() => handleInitializePairing()}
        confirmDisabled={pairingSubmitting === "init_pairing"}
        requireExplicitAction
      />
      <FirmwareFlashDialog
        open={flashDialogOpen}
        onClose={() => setFlashDialogOpen(false)}
      />
    </>
  );
}
