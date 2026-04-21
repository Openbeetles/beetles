import MenuItem from "@mui/material/MenuItem";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import FormControlLabel from "@mui/material/FormControlLabel";
import Switch from "@mui/material/Switch";
import TextField from "@mui/material/TextField";
import SaveRounded from "@mui/icons-material/SaveRounded";
import {
  FormLoadingSkeleton,
  PanelStateBlock,
  PanelStateLoading,
  FormSectionSubCollapsible,
  InlineAlert,
  SaveFeedback,
  SettingsRow,
} from "../components/form";
import { Os3dIcon } from "../components/Os3dIcon";
import { SettingsSection } from "../components/SettingsSection";
import { OS_ICON_NAV } from "../config/osIcons";
import {
  PAGE_COLUMN_FILL_SX,
  PAGE_STACK_OUTER_SX,
} from "../theme/panelStyles";
import { useConfig } from "../hooks/useConfig";
import { useConfigPageLoad } from "../hooks/useConfigPageLoad";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { useSaveFeedback } from "../hooks/useSaveFeedback";
import { useUnsaved } from "../hooks/useUnsaved";
import { useRevealedPasswordFields } from "../hooks/useRevealedPassword";
import { useSyncedNullableState } from "../hooks/useSyncedNullableState";
import { ENABLED_CHANNEL_OPTIONS } from "../types/appConfig";
import type { AppConfig } from "../types/appConfig";

const MAX_LEN = 64;
const MAX_DINGTALK = 512;
const MAX_WECOM_TOUSER = 128;
const TG_ACTIVATION_OPTIONS = ["mention", "always"] as const;

function validateChannels(
  config: AppConfig,
  t: (k: string) => string,
): string | null {
  if (
    config.tg_group_activation !== "mention" &&
    config.tg_group_activation !== "always"
  )
    return t("config.validation.tgGroupActivation");
  if (config.dingtalk_webhook_url.length > MAX_DINGTALK)
    return t("config.validation.dingtalkMax512");
  if (config.wecom_default_touser.length > MAX_WECOM_TOUSER)
    return t("config.validation.wecomTouserMax128");
  return null;
}

export function ChannelsConfigPage() {
  const { t } = useTranslation();
  const { ready, deviceConnected, hasPairing, connectionChecking } = useDeviceApi();
  const { config, loadConfig, saveChannels, loading, error } = useConfig();
  const { setDirty } = useUnsaved();
  const [form, setForm] = useSyncedNullableState<AppConfig>(config);
  const saveFeedback = useSaveFeedback(t);
  useConfigPageLoad({
    hasConfig: config !== null,
    loading,
    loadConfig,
    canLoad: ready && deviceConnected,
  });

  const { isRevealed, getRevealHandlers } = useRevealedPasswordFields();

  const update = (key: keyof AppConfig, value: string | number | boolean) => {
    setDirty(true);
    setForm((prev) => (prev ? { ...prev, [key]: value } : null));
  };

  const handleSave = async () => {
    if (!config || !form) return;
    const err = validateChannels(form, t);
    if (err) {
      saveFeedback.fail(err);
      return;
    }
    const segment = {
      enabled_channel: form.enabled_channel ?? "",
      tg_token: form.tg_token,
      tg_allowed_chat_ids: form.tg_allowed_chat_ids,
      feishu_app_id: form.feishu_app_id,
      feishu_app_secret: form.feishu_app_secret,
      feishu_verification_token: form.feishu_verification_token,
      feishu_encrypt_key: form.feishu_encrypt_key,
      feishu_allowed_chat_ids: form.feishu_allowed_chat_ids,
      dingtalk_webhook_url: form.dingtalk_webhook_url ?? "",
      wecom_corp_id: form.wecom_corp_id,
      wecom_corp_secret: form.wecom_corp_secret,
      wecom_agent_id: form.wecom_agent_id,
      wecom_default_touser: form.wecom_default_touser,
      wecom_token: form.wecom_token,
      wecom_encoding_aes_key: form.wecom_encoding_aes_key,
      dingtalk_app_secret: form.dingtalk_app_secret,
      qq_channel_app_id: form.qq_channel_app_id,
      qq_channel_secret: form.qq_channel_secret,
      webhook_enabled: form.webhook_enabled,
      webhook_token: form.webhook_token,
    };
    saveFeedback.begin();
    const result = await saveChannels(segment);
    saveFeedback.finishFromResult(result);
    if (result.ok) setDirty(false);
  };

  if (loading && !config) {
    return (
      <Box sx={PAGE_COLUMN_FILL_SX}>
        <SettingsSection
          pinHeader
          surfaceTone="loading"
          sx={{ flex: 1, minHeight: 0 }}
          icon={<Os3dIcon src={OS_ICON_NAV["/channels-config"]} />}
          label={t("config.sectionChannels")}
        >
          <PanelStateLoading>
            <FormLoadingSkeleton />
          </PanelStateLoading>
        </SettingsSection>
      </Box>
    );
  }

  const saveDisabled = saveFeedback.status === "saving" || !form;
  const showConnectionLoading =
    !form && !loading && ready && connectionChecking && !deviceConnected;
  const showConnectState =
    !form && !loading && !showConnectionLoading && (!ready || !deviceConnected);
  const showPairingState =
    !form && !loading && ready && deviceConnected && !hasPairing;
  const inlineError = showConnectState || showPairingState ? null : error;

  return (
    <Box sx={PAGE_STACK_OUTER_SX}>
      <InlineAlert message={inlineError} onRetry={loadConfig} />
      <SettingsSection
        pinHeader
        sx={{ flex: 1, minHeight: 0 }}
        icon={<Os3dIcon src={OS_ICON_NAV["/channels-config"]} />}
        label={t("config.sectionChannels")}
        description={t("config.sectionChannelsDesc")}
        accessory={
          <Button
            size="small"
            variant="contained"
            startIcon={<SaveRounded />}
            onClick={handleSave}
            disabled={saveDisabled}
            title={!form ? t("config.hintSaveNeedDevice") : undefined}
            sx={{ borderRadius: "var(--radius-control)" }}
          >
            {saveFeedback.status === "saving" ? t("common.saving") : t("common.save")}
          </Button>
        }
        belowTitleRow={
          saveFeedback.status === "ok" || saveFeedback.status === "fail" ? (
            <SaveFeedback
              placement="belowTitle"
              status={saveFeedback.status}
              message={saveFeedback.status === "ok" ? t("common.saveOk") : saveFeedback.error}
              autoDismissMs={3000}
              onDismiss={saveFeedback.dismiss}
            />
          ) : null
        }
      >
        {showConnectionLoading ? (
          <PanelStateLoading>
            <FormLoadingSkeleton />
          </PanelStateLoading>
        ) : showConnectState ? (
          <PanelStateBlock
            tone="neutral"
            icon={<Os3dIcon src={OS_ICON_NAV["/channels-config"]} variant="inline" />}
            title={ready ? t("device.connectFirst") : t("device.bannerNeedDevice")}
            description={t("config.connectDesc")}
          />
        ) : showPairingState ? (
          <PanelStateBlock
            tone="neutral"
            icon={<Os3dIcon src={OS_ICON_NAV["/channels-config"]} variant="inline" />}
            title={t("device.pairingCodeRequired")}
            description={t("config.needPairingDesc")}
          />
        ) : !form ? (
          <PanelStateBlock
            tone="neutral"
            icon={<Os3dIcon src={OS_ICON_NAV["/channels-config"]} variant="inline" />}
            title={t("config.unavailableTitle")}
            description={t("config.unavailableDesc")}
          />
        ) : (
          <>
        <SettingsRow
          label={t("config.enabledChannel")}
          description={t("config.enabledChannelHelp")}
        >
          <TextField
            select
            hiddenLabel
            fullWidth
            value={form.enabled_channel ?? ""}
            onChange={(e) => update("enabled_channel", e.target.value)}
            aria-label={t("config.enabledChannel")}
            slotProps={{
              inputLabel: { shrink: true },
            }}
          >
            {ENABLED_CHANNEL_OPTIONS.map((opt) => (
              <MenuItem key={opt.value || "none"} value={opt.value}>
                {t(opt.labelKey)}
              </MenuItem>
            ))}
          </TextField>
        </SettingsRow>
        <FormSectionSubCollapsible
          title="Telegram"
          defaultOpen={!form.enabled_channel || form.enabled_channel === "telegram"}
        >
          <TextField
            label={t("config.tgToken")}
            value={form.tg_token}
            onChange={(e) => update("tg_token", e.target.value)}
            type={isRevealed("tg_token") ? "text" : "password"}
            fullWidth
            slotProps={{
              htmlInput: {
                maxLength: MAX_LEN,
                style: { fontFamily: "var(--font-mono)" },
                ...getRevealHandlers("tg_token"),
              },
            }}
          />
          <TextField
            label={t("config.tgAllowedChatIds")}
            value={form.tg_allowed_chat_ids}
            onChange={(e) => update("tg_allowed_chat_ids", e.target.value)}
            fullWidth
            helperText={t("config.tgAllowedChatIdsHelp")}
            slotProps={{ htmlInput: { maxLength: MAX_LEN * 4 } }}
          />
          <SettingsRow
            label={t("config.tgGroupActivation")}
            divider={false}
          >
            <TextField
              select
              hiddenLabel
              fullWidth
              value={form.tg_group_activation}
              onChange={(e) => update("tg_group_activation", e.target.value)}
              aria-label={t("config.tgGroupActivation")}
              slotProps={{
                inputLabel: { shrink: true },
              }}
            >
              {TG_ACTIVATION_OPTIONS.map((opt) => (
                <MenuItem key={opt} value={opt}>
                  {t(`config.tgGroupActivation_${opt}`)}
                </MenuItem>
              ))}
            </TextField>
          </SettingsRow>
        </FormSectionSubCollapsible>

        <FormSectionSubCollapsible
          title={t("config.feishu")}
          defaultOpen={form.enabled_channel === "feishu"}
        >
          <TextField
            label={t("config.feishuAppId")}
            value={form.feishu_app_id}
            onChange={(e) => update("feishu_app_id", e.target.value)}
            fullWidth
            slotProps={{ htmlInput: { maxLength: MAX_LEN } }}
          />
          <TextField
            label={t("config.feishuAppSecret")}
            value={form.feishu_app_secret}
            onChange={(e) => update("feishu_app_secret", e.target.value)}
            type={isRevealed("feishu_app_secret") ? "text" : "password"}
            fullWidth
            slotProps={{
              htmlInput: {
                maxLength: MAX_LEN,
                ...getRevealHandlers("feishu_app_secret"),
              },
            }}
          />
          <TextField
            label={t("config.feishuVerificationToken")}
            value={form.feishu_verification_token}
            onChange={(e) => update("feishu_verification_token", e.target.value)}
            fullWidth
            slotProps={{ htmlInput: { maxLength: MAX_LEN } }}
          />
          <TextField
            label={t("config.feishuEncryptKey")}
            value={form.feishu_encrypt_key}
            onChange={(e) => update("feishu_encrypt_key", e.target.value)}
            type={isRevealed("feishu_encrypt_key") ? "text" : "password"}
            fullWidth
            slotProps={{
              htmlInput: {
                maxLength: MAX_LEN,
                ...getRevealHandlers("feishu_encrypt_key"),
              },
            }}
          />
          <TextField
            label={t("config.feishuAllowedChatIds")}
            value={form.feishu_allowed_chat_ids}
            onChange={(e) => update("feishu_allowed_chat_ids", e.target.value)}
            fullWidth
            slotProps={{ htmlInput: { maxLength: MAX_LEN * 4 } }}
          />
        </FormSectionSubCollapsible>

        <FormSectionSubCollapsible
          title={t("config.dingtalk")}
          defaultOpen={form.enabled_channel === "dingtalk"}
        >
          <TextField
            label={t("config.dingtalkWebhookUrl")}
            value={form.dingtalk_webhook_url}
            onChange={(e) => update("dingtalk_webhook_url", e.target.value)}
            type="url"
            fullWidth
            helperText={`${form.dingtalk_webhook_url.length}/${MAX_DINGTALK}`}
            slotProps={{
              htmlInput: {
                maxLength: MAX_DINGTALK,
                style: { fontFamily: "var(--font-mono)" },
              },
            }}
          />
          <TextField
            label={t("config.dingtalkAppSecret")}
            value={form.dingtalk_app_secret}
            onChange={(e) => update("dingtalk_app_secret", e.target.value)}
            type={isRevealed("dingtalk_app_secret") ? "text" : "password"}
            fullWidth
            helperText={t("config.dingtalkAppSecretHelp")}
            slotProps={{
              htmlInput: {
                maxLength: MAX_LEN,
                ...getRevealHandlers("dingtalk_app_secret"),
              },
            }}
          />
        </FormSectionSubCollapsible>

        <FormSectionSubCollapsible
          title={t("config.wecom")}
          defaultOpen={form.enabled_channel === "wecom"}
        >
          <TextField
            label={t("config.wecomCorpId")}
            value={form.wecom_corp_id}
            onChange={(e) => update("wecom_corp_id", e.target.value)}
            fullWidth
            slotProps={{ htmlInput: { maxLength: MAX_LEN } }}
          />
          <TextField
            label={t("config.wecomCorpSecret")}
            value={form.wecom_corp_secret}
            onChange={(e) => update("wecom_corp_secret", e.target.value)}
            type={isRevealed("wecom_corp_secret") ? "text" : "password"}
            fullWidth
            slotProps={{
              htmlInput: {
                maxLength: MAX_LEN,
                ...getRevealHandlers("wecom_corp_secret"),
              },
            }}
          />
          <TextField
            label={t("config.wecomAgentId")}
            value={form.wecom_agent_id}
            onChange={(e) => update("wecom_agent_id", e.target.value)}
            fullWidth
            slotProps={{ htmlInput: { maxLength: MAX_LEN } }}
          />
          <TextField
            label={t("config.wecomDefaultTouser")}
            value={form.wecom_default_touser}
            onChange={(e) => update("wecom_default_touser", e.target.value)}
            fullWidth
            helperText={`${t("config.wecomDefaultTouserHelp")} · ${form.wecom_default_touser.length}/${MAX_WECOM_TOUSER}`}
            slotProps={{ htmlInput: { maxLength: MAX_WECOM_TOUSER } }}
          />
          <TextField
            label={t("config.wecomToken")}
            value={form.wecom_token}
            onChange={(e) => update("wecom_token", e.target.value)}
            fullWidth
            slotProps={{ htmlInput: { maxLength: MAX_LEN } }}
          />
          <TextField
            label={t("config.wecomEncodingAesKey")}
            value={form.wecom_encoding_aes_key}
            onChange={(e) => update("wecom_encoding_aes_key", e.target.value)}
            type={isRevealed("wecom_encoding_aes_key") ? "text" : "password"}
            fullWidth
            slotProps={{
              htmlInput: {
                maxLength: MAX_LEN,
                ...getRevealHandlers("wecom_encoding_aes_key"),
              },
            }}
          />
        </FormSectionSubCollapsible>

        <FormSectionSubCollapsible
          title={t("config.qqChannel")}
          defaultOpen={form.enabled_channel === "qq_channel"}
        >
          <TextField
            label={t("config.qqChannelAppId")}
            value={form.qq_channel_app_id}
            onChange={(e) => update("qq_channel_app_id", e.target.value)}
            fullWidth
            slotProps={{ htmlInput: { maxLength: MAX_LEN } }}
          />
          <TextField
            label={t("config.qqChannelSecret")}
            value={form.qq_channel_secret}
            onChange={(e) => update("qq_channel_secret", e.target.value)}
            type={isRevealed("qq_channel_secret") ? "text" : "password"}
            fullWidth
            slotProps={{
              htmlInput: {
                maxLength: MAX_LEN,
                ...getRevealHandlers("qq_channel_secret"),
              },
            }}
          />
        </FormSectionSubCollapsible>

        <FormSectionSubCollapsible title="Webhook" defaultOpen={form.webhook_enabled}>
          <FormControlLabel
            control={
              <Switch
                checked={form.webhook_enabled}
                onChange={(e) => update("webhook_enabled", e.target.checked)}
                sx={{
                  "& .MuiSwitch-switchBase": {
                    borderRadius: "var(--radius-control)",
                  },
                }}
              />
            }
            label={t("config.webhookEnabled")}
          />
          <TextField
            label={t("config.webhookToken")}
            value={form.webhook_token}
            onChange={(e) => update("webhook_token", e.target.value)}
            type={isRevealed("webhook_token") ? "text" : "password"}
            fullWidth
            slotProps={{
              htmlInput: {
                maxLength: MAX_LEN,
                style: { fontFamily: "var(--font-mono)" },
                ...getRevealHandlers("webhook_token"),
              },
            }}
          />
        </FormSectionSubCollapsible>
          </>
        )}
      </SettingsSection>
    </Box>
  );
}
