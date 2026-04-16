import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Checkbox from "@mui/material/Checkbox";
import FormControl from "@mui/material/FormControl";
import FormControlLabel from "@mui/material/FormControlLabel";
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
  TEXT_BODY_TERTIARY_SX,
} from "../theme/panelStyles";
import { useDeviceApi } from "../hooks/useDeviceApi";
import {
  PanelStateLoading,
  SectionLoadingSkeleton,
} from "./form";
import { localizeAccountProviderName } from "../i18n/providerDisplay";
import {
  localizeProviderField,
  localizeProviderFieldLabel,
} from "../i18n/providerFields";
import { ProviderFieldInput } from "./ProviderFieldInput";
import { errorMessage, withTimeout } from "../util/withTimeout";

const ACCOUNT_REQUEST_TIMEOUT_MS = 15_000;

const IDENTITY_ORDER: AccountIdentityClass[] = [
  "work",
  "personal",
  "family",
  "shared",
  "other",
];

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
  const [identityClass, setIdentityClass] =
    useState<AccountIdentityClass>("work");
  const [enabledCaps, setEnabledCaps] = useState<AccountCapability[]>([]);
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
        setCatalogError(res.error ?? t("accounts.providersLoadFailed"));
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
          field.key !== "identity_class" &&
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
        setEnabledCaps([]);
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
      setEnabledCaps([...selectedProvider.capabilities]);
      setFieldValues(next);
    });
  }, [providerAccountFields, providerConfigFields, selectedProvider]);

  const toggleCap = (c: AccountCapability) => {
    setEnabledCaps((prev) =>
      prev.includes(c) ? prev.filter((x) => x !== c) : [...prev, c],
    );
  };

  const handleSubmit = async () => {
    if (!hasPairing || !selectedProvider) return;
    if (enabledCaps.length === 0) {
      setCreateError(t("accounts.createCapabilityRequired"));
      return;
    }
    const invalid = localizedProviderInputFields.filter(
      (f) => f.required && !(fieldValues[f.key] ?? "").trim(),
    );
    if (invalid.length > 0) {
      setCreateError(
        t("accounts.createRequiredFields", {
          fields: invalid
            .map((f) => localizeProviderFieldLabel(t, f.key, f.label))
            .join(", "),
        }),
      );
      return;
    }

    let externalAccountId: string | undefined;
    const configFields: Record<string, string> = {};
    for (const f of providerAccountFields) {
      const raw = (fieldValues[f.key] ?? "").trim();
      if (f.key === "external_account_id") {
        if (raw) externalAccountId = raw;
      }
    }
    for (const f of providerConfigFields) {
      const raw = (fieldValues[f.key] ?? "").trim();
      if (raw) configFields[f.key] = raw;
    }

    const body: AccountUpsertRequest = {
      account: {
        provider_kind: selectedProvider.provider_kind,
        external_account_id: externalAccountId,
        account_label: accountLabelInput.trim() || undefined,
        identity_class: identityClass,
        enabled_capabilities: enabledCaps,
      },
    };
    if (Object.keys(configFields).length > 0) {
      body.config = { fields: configFields };
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
        setCreateError(res.error ?? t("accounts.createFailed"));
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
      <Typography color="error" variant="body2">
        {catalogError}
      </Typography>
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
                {localizeAccountProviderName(t, p.provider_kind)}{" "}
                ({p.provider_kind})
              </MenuItem>
            ))}
          </Select>
        </FormControl>
      </Box>

      <Box sx={{ ...CONFIG_PANEL_SX, p: PANEL_SECTION_PADDING }}>
        <Stack spacing={3}>
          <TextField
            fullWidth
            label={t("accounts.accountLabelOptional")}
            value={accountLabelInput}
            onChange={(e) => setAccountLabelInput(e.target.value)}
            inputProps={{ autoComplete: "off" }}
          />
          <FormControl fullWidth>
            <InputLabel id="account-create-identity-label">
              {t("accounts.identityLabel")}
            </InputLabel>
            <Select
              labelId="account-create-identity-label"
              label={t("accounts.identityLabel")}
              value={identityClass}
              onChange={(e) =>
                setIdentityClass(e.target.value as AccountIdentityClass)
              }
            >
              {IDENTITY_ORDER.map((id) => (
                <MenuItem key={id} value={id}>
                  {t(`accounts.identity.${id}`)}
                </MenuItem>
              ))}
            </Select>
          </FormControl>
        </Stack>
      </Box>

      <Box sx={{ ...CONFIG_PANEL_SX, p: PANEL_SECTION_PADDING }}>
        <Typography
          variant="subtitle2"
          color="text.secondary"
          sx={{ mb: 1.5, display: "block", fontWeight: 600 }}
        >
          {t("accounts.enabledCapabilities")}
        </Typography>
        <Stack spacing={1.25}>
          {selectedProvider.capabilities.map((c) => (
            <FormControlLabel
              key={c}
              control={
                <Checkbox
                  checked={enabledCaps.includes(c)}
                  onChange={() => toggleCap(c)}
                />
              }
              label={t(`accounts.capability.${c}`)}
            />
          ))}
        </Stack>
        <Typography variant="caption" sx={{ ...TEXT_BODY_TERTIARY_SX, mt: 2, display: "block" }}>
          {t("accounts.enabledCapabilitiesHint")}
        </Typography>
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
