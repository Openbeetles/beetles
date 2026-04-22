import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import FormControl from "@mui/material/FormControl";
import FormHelperText from "@mui/material/FormHelperText";
import InputLabel from "@mui/material/InputLabel";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import Stack from "@mui/material/Stack";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import type {
  AccountCapability,
  AccountIdentityClass,
  AccountUpsertRequest,
  ProviderCatalogItem,
} from "../types/accountConfig";
import {
  CONFIG_PANEL_SX,
  DIALOG_FORM_SCROLL_WELL_SX,
  DIALOG_FORM_SUBMIT_BAR_SX,
  PANEL_SECTION_PADDING,
} from "../theme/panelStyles";
import { useDeviceApi } from "../hooks/useDeviceApi";
import {
  PageLoadErrorState,
  PanelStateLoading,
  SectionLoadingSkeleton,
} from "./form";
import { localizeAccountProviderName } from "../i18n/providerDisplay";
import {
  localizeProviderField,
  localizeProviderFieldLabel,
} from "../i18n/providerFields";
import { translateApiError } from "../i18n/apiErrors";
import { ProviderFieldInput } from "./ProviderFieldInput";
import { errorMessage, withTimeout } from "../util/withTimeout";

const ACCOUNT_REQUEST_TIMEOUT_MS = 15_000;

export type AccountCapabilityFilter = "all" | AccountCapability;

export interface AccountCreateFormProps {
  capabilityFilter: AccountCapabilityFilter;
  onCreated: (accountKey: string) => void;
}

export function AccountCreateForm({
  capabilityFilter,
  onCreated,
}: AccountCreateFormProps) {
  const { t } = useTranslation();
  const { api, ready, hasPairing } = useDeviceApi();

  const [catalog, setCatalog] = useState<ProviderCatalogItem[]>([]);
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [catalogError, setCatalogError] = useState("");
  const [providerKind, setProviderKind] = useState("");
  const [accountLabelInput, setAccountLabelInput] = useState("");
  const [fieldValues, setFieldValues] = useState<Record<string, string>>({});
  const [createBusy, setCreateBusy] = useState(false);
  const [createError, setCreateError] = useState("");

  const loadCatalog = useCallback(async () => {
    if (!ready) return;
    setCatalogLoading(true);
    setCatalogError("");
    const cap =
      capabilityFilter === "all" ? undefined : capabilityFilter;
    try {
      const res = await withTimeout(
        api.config.accounts.getProviders(cap),
        ACCOUNT_REQUEST_TIMEOUT_MS,
        t("accounts.requestTimedOut"),
      );
      if (res.ok && res.data) {
        setCatalog(res.data.items);
      } else {
        setCatalog([]);
        setCatalogError(translateApiError(t, res.error, "accounts.providersLoadFailed"));
      }
    } catch (error) {
      setCatalog([]);
      setCatalogError(errorMessage(error, t("accounts.providersLoadFailed")));
    }
    setCatalogLoading(false);
  }, [api.config.accounts, capabilityFilter, ready, t]);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- initial catalog fetch must start on mount/provider-filter change
    void loadCatalog();
  }, [loadCatalog]);

  const selectedProvider = useMemo(
    () => catalog.find((p) => p.provider_kind === providerKind) ?? null,
    [catalog, providerKind],
  );
  const providerAccountFields = useMemo(
    () =>
      selectedProvider?.account_fields.filter(
        (field) =>
          field.key !== "account_label" &&
          field.key !== "enabled_capabilities",
      ) ?? [],
    [selectedProvider],
  );
  const providerConfigFields = useMemo(
    () => selectedProvider?.config_fields ?? [],
    [selectedProvider],
  );
  const localizedProviderAccountFields = useMemo(
    () => providerAccountFields.map((field) => localizeProviderField(t, field)),
    [providerAccountFields, t],
  );
  const localizedProviderConfigFields = useMemo(
    () => providerConfigFields.map((field) => localizeProviderField(t, field)),
    [providerConfigFields, t],
  );
  const localizedProviderInputFields = useMemo(
    () => [...localizedProviderAccountFields, ...localizedProviderConfigFields],
    [localizedProviderAccountFields, localizedProviderConfigFields],
  );

  useEffect(() => {
    if (catalog.length === 0) {
      queueMicrotask(() => setProviderKind(""));
      return;
    }
    queueMicrotask(() =>
      setProviderKind((prev) => {
        if (prev && catalog.some((p) => p.provider_kind === prev)) return prev;
        return catalog[0]!.provider_kind;
      }),
    );
  }, [catalog]);

  useEffect(() => {
    if (!selectedProvider) {
      queueMicrotask(() => {
        setFieldValues({});
      });
      return;
    }
    const next: Record<string, string> = {};
    for (const f of providerAccountFields) {
      if (f.default_value) next[f.key] = f.default_value;
      else if (f.default_values?.length)
        next[f.key] = f.default_values.join("\n");
      else next[f.key] = "";
    }
    for (const f of providerConfigFields) {
      if (f.default_value) next[f.key] = f.default_value;
      else next[f.key] = "";
    }
    queueMicrotask(() => {
      setFieldValues(next);
    });
  }, [providerAccountFields, providerConfigFields, selectedProvider]);

  const handleSubmit = async () => {
    if (!hasPairing || !selectedProvider) return;
    const invalid = localizedProviderInputFields.filter(
      (f) => f.required && !(fieldValues[f.key] ?? "").trim(),
    );
    if (invalid.length > 0) {
      setCreateError(
        t("accounts.createRequiredFields", {
          fields: invalid
            .map((f) => localizeProviderFieldLabel(t, f))
            .join(", "),
        }),
      );
      return;
    }

    let externalAccountId: string | undefined;
    let identityClass: AccountIdentityClass | undefined;
    const metadataFields: Record<string, string> = {};
    let accessToken: string | undefined;
    let refreshToken: string | undefined;
    let tokenEndpoint: string | undefined;
    let mailUsername: string | undefined;
    let mailFromAddress: string | undefined;
    let imapHost: string | undefined;
    let imapPort: string | undefined;
    let imapTls: boolean | undefined;
    let smtpHost: string | undefined;
    let smtpPort: string | undefined;
    let smtpTls: boolean | undefined;
    for (const f of providerAccountFields) {
      const raw = (fieldValues[f.key] ?? "").trim();
      if (f.key === "external_account_id") {
        if (raw) externalAccountId = raw;
        continue;
      }
      if (f.key === "identity_class") {
        if (raw) {
          identityClass = raw as AccountIdentityClass;
        }
        continue;
      }
      if (raw) {
        metadataFields[f.key] = raw;
      }
    }
    for (const f of providerConfigFields) {
      const raw = (fieldValues[f.key] ?? "").trim();
      if (!raw) continue;
      switch (f.key) {
        case "access_token":
          accessToken = raw;
          break;
        case "refresh_token":
          refreshToken = raw;
          break;
        case "token_endpoint":
          tokenEndpoint = raw;
          break;
        case "mail_username":
          mailUsername = raw;
          break;
        case "mail_from_address":
          mailFromAddress = raw;
          break;
        case "imap_host":
          imapHost = raw;
          break;
        case "imap_port":
          imapPort = raw;
          break;
        case "imap_tls":
          imapTls = raw === "true";
          break;
        case "smtp_host":
          smtpHost = raw;
          break;
        case "smtp_port":
          smtpPort = raw;
          break;
        case "smtp_tls":
          smtpTls = raw === "true";
          break;
        default:
          metadataFields[f.key] = raw;
          break;
      }
    }
    if (!identityClass) {
      setCreateError(
        t("accounts.createRequiredFields", {
          fields: t("accounts.identityLabel"),
        }),
      );
      return;
    }
    const body: AccountUpsertRequest = {
      provider_kind: selectedProvider.provider_kind,
      identity_class: identityClass,
      account_label: accountLabelInput.trim() || undefined,
      access_token: accessToken,
      refresh_token: refreshToken,
      token_endpoint: tokenEndpoint,
      mail_username: mailUsername,
      mail_from_address: mailFromAddress,
      imap_host: imapHost,
      imap_port: imapPort,
      imap_tls: imapTls,
      smtp_host: smtpHost,
      smtp_port: smtpPort,
      smtp_tls: smtpTls,
    };
    if (selectedProvider.capabilities.length === 1) {
      body.capability = selectedProvider.capabilities[0];
    }
    if (externalAccountId) {
      body.external_account_id = externalAccountId;
    }
    if (Object.keys(metadataFields).length > 0) {
      body.metadata = metadataFields;
    }

    setCreateBusy(true);
    setCreateError("");
    try {
      const res = await withTimeout(
        api.config.accounts.create(body),
        ACCOUNT_REQUEST_TIMEOUT_MS,
        t("accounts.requestTimedOut"),
      );
      setCreateBusy(false);
      if (res.ok && res.data) {
        onCreated(res.data.account.account_key);
      } else {
        setCreateError(translateApiError(t, res.error, "accounts.createFailed"));
      }
    } catch (error) {
      setCreateBusy(false);
      setCreateError(errorMessage(error, t("accounts.createFailed")));
    }
  };

  if (catalogLoading) {
    return (
      <PanelStateLoading>
        <SectionLoadingSkeleton />
      </PanelStateLoading>
    );
  }

  if (catalogError) {
    return (
      <PageLoadErrorState message={catalogError} onRetry={() => void loadCatalog()} />
    );
  }

  if (!selectedProvider) {
    return (
      <Typography variant="body2" color="text.secondary">
        {t("accounts.noProviders")}
      </Typography>
    );
  }

  const submitBar = (
    <Box sx={DIALOG_FORM_SUBMIT_BAR_SX}>
      <Button
        fullWidth
        size="large"
        variant="contained"
        disabled={!hasPairing || createBusy}
        onClick={() => void handleSubmit()}
      >
        {createBusy ? t("accounts.createBusy") : t("accounts.createSubmit")}
      </Button>
      {!hasPairing ? (
        <FormHelperText sx={{ mx: 0 }}>
          {t("accounts.createNeedsPairing")}
        </FormHelperText>
      ) : null}
    </Box>
  );

  return (
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
      {createError ? (
        <Typography color="error" variant="body2">
          {createError}
        </Typography>
      ) : null}

      <Box sx={{ ...CONFIG_PANEL_SX, p: PANEL_SECTION_PADDING }}>
        <FormControl fullWidth>
          <InputLabel id="account-create-provider-label">
            {t("accounts.selectProvider")}
          </InputLabel>
          <Select
            labelId="account-create-provider-label"
            label={t("accounts.selectProvider")}
            value={providerKind}
            onChange={(e) => setProviderKind(e.target.value)}
          >
            {catalog.map((p) => (
              <MenuItem key={p.provider_kind} value={p.provider_kind}>
                {localizeAccountProviderName(t, {
                  providerKind: p.provider_kind,
                  displayNameKey: p.display_name_key,
                })}
              </MenuItem>
            ))}
          </Select>
        </FormControl>
      </Box>

      <Box sx={{ ...CONFIG_PANEL_SX, p: PANEL_SECTION_PADDING }}>
        <Stack spacing={0}>
          <TextField
            fullWidth
            label={t("accounts.accountLabelOptional")}
            value={accountLabelInput}
            onChange={(e) => setAccountLabelInput(e.target.value)}
            inputProps={{ autoComplete: "off" }}
          />
        </Stack>
      </Box>

      {localizedProviderInputFields.length > 0 ? (
        <Box sx={{ ...CONFIG_PANEL_SX, p: PANEL_SECTION_PADDING }}>
          <Typography
            variant="subtitle2"
            color="text.secondary"
            sx={{ mb: 1.5, display: "block", fontWeight: 600 }}
          >
            {t("accounts.providerFields")}
          </Typography>
          <Stack spacing={3}>
            {localizedProviderAccountFields.map((field) => (
              <ProviderFieldInput
                key={field.key}
                field={field}
                value={fieldValues[field.key] ?? ""}
                onChange={(nextValue) =>
                  setFieldValues((prev) => ({
                    ...prev,
                    [field.key]: nextValue,
                  }))
                }
              />
            ))}
            {localizedProviderConfigFields.map((field) => (
              <ProviderFieldInput
                key={field.key}
                field={field}
                value={fieldValues[field.key] ?? ""}
                onChange={(nextValue) =>
                  setFieldValues((prev) => ({
                    ...prev,
                    [field.key]: nextValue,
                  }))
                }
              />
            ))}
          </Stack>
        </Box>
      ) : null}
      </Stack>
      {submitBar}
    </Stack>
  );
}
