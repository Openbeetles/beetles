import MenuItem from "@mui/material/MenuItem";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Switch from "@mui/material/Switch";
import TextField from "@mui/material/TextField";
import SaveRounded from "@mui/icons-material/SaveRounded";
import {
  FormLoadingSkeleton,
  FormGrid,
  FormSwitchRow,
  PanelStateBlock,
  PanelStateLoading,
  FormSectionSubCollapsible,
  InlineAlert,
  PageLoadErrorState,
  SaveFeedback,
  SettingsRow,
  splitPageErrorState,
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
import {
  enabledChannelLabelKey,
  type ChannelsConfigView,
} from "../types/appConfig";

const MAX_LEN = 64;
const MAX_URL = 512;
const CHANNELS_CONFIG_DIRTY_OWNER = "channels-config";
const TG_ACTIVATION_OPTIONS = ["mention", "always"] as const;
const DISABLED_CHANNEL_SELECT_VALUE = "__beetle_disabled_channel__";

function channelConfigValueToSelectValue(channelId: string): string {
  return channelId.trim().length === 0
    ? DISABLED_CHANNEL_SELECT_VALUE
    : channelId;
}

function channelSelectValueToConfigValue(value: string): string {
  return value === DISABLED_CHANNEL_SELECT_VALUE ? "" : value;
}

function validateChannels(
  config: ChannelsConfigView,
  t: (k: string) => string,
): string | null {
  if (
    config.tg_group_activation !== "mention" &&
    config.tg_group_activation !== "always"
  )
    return t("config.validation.tgGroupActivation");
  if (config.wecom_ws_url.length > MAX_URL)
    return t("config.validation.urlMax512");
  return null;
}

export function ChannelsConfigPage() {
  const { t } = useTranslation();
  const { ready, deviceConnected, canAccessProtectedApis, connectionChecking } = useDeviceApi();
  const {
    channelsConfig,
    loadChannelsConfig,
    saveChannels,
    channelsLoading,
    channelsError,
  } = useConfig();
  const { setDirtyFor, clearDirtyFor } = useUnsaved();
  const [form, setForm] = useSyncedNullableState<ChannelsConfigView>(channelsConfig);
  const saveFeedback = useSaveFeedback(t);
  useConfigPageLoad({
    hasConfig: channelsConfig !== null,
    loading: channelsLoading,
    loadConfig: loadChannelsConfig,
    canLoad: ready && deviceConnected,
  });

  const { isRevealed, getRevealHandlers } = useRevealedPasswordFields();

  const markDirty = () => setDirtyFor(CHANNELS_CONFIG_DIRTY_OWNER, true);
  const markClean = () => setDirtyFor(CHANNELS_CONFIG_DIRTY_OWNER, false);

  useEffect(() => {
    return () => clearDirtyFor(CHANNELS_CONFIG_DIRTY_OWNER);
  }, [clearDirtyFor]);

  const update = (
    key: keyof ChannelsConfigView,
    value: string | number | boolean,
  ) => {
    markDirty();
    setForm((prev) => (prev ? { ...prev, [key]: value } : null));
  };

  const handleSave = async () => {
    if (!channelsConfig || !form) return;
    const err = validateChannels(form, t);
    if (err) {
      saveFeedback.fail(err);
      return;
    }
    const segment = {
      enabled_channel: form.enabled_channel ?? "",
      tg_token: form.tg_token,
      tg_allowed_chat_ids: form.tg_allowed_chat_ids,
      tg_group_activation: form.tg_group_activation,
      feishu_app_id: form.feishu_app_id,
      feishu_app_secret: form.feishu_app_secret,
      feishu_allowed_chat_ids: form.feishu_allowed_chat_ids,
      dingtalk_client_id: form.dingtalk_client_id,
      dingtalk_client_secret: form.dingtalk_client_secret,
      wecom_bot_id: form.wecom_bot_id,
      wecom_bot_secret: form.wecom_bot_secret,
      wecom_ws_url: form.wecom_ws_url,
      qq_channel_app_id: form.qq_channel_app_id,
      qq_channel_secret: form.qq_channel_secret,
      webhook_enabled: form.webhook_enabled,
      webhook_token: form.webhook_token,
    };
    saveFeedback.begin();
    const result = await saveChannels(segment);
    saveFeedback.finishFromResult(result);
    if (result.ok) markClean();
  };

  if (channelsLoading && !channelsConfig) {
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
  const availableChannels = form
    ? new Set(form.available_channels.filter((value) => value.trim().length > 0))
    : new Set<string>();
  const selectedChannelValue = form
    ? channelConfigValueToSelectValue(form.enabled_channel)
    : DISABLED_CHANNEL_SELECT_VALUE;
  const showConnectionLoading =
    !form && !channelsLoading && ready && connectionChecking && !deviceConnected;
  const showConnectState =
    !form && !channelsLoading && !showConnectionLoading && (!ready || !deviceConnected);
  const showPairingState =
    !form && !channelsLoading && ready && deviceConnected && !canAccessProtectedApis;
  const loadErrorState = splitPageErrorState({
    hasData: Boolean(form),
    loading: channelsLoading,
    error: channelsError,
    suppress: showConnectState || showPairingState || showConnectionLoading,
  });

  return (
    <Box sx={PAGE_STACK_OUTER_SX}>
      <InlineAlert message={loadErrorState.inlineError} onRetry={loadChannelsConfig} />
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
        ) : loadErrorState.blockingError ? (
          <PageLoadErrorState
            message={loadErrorState.blockingError}
            onRetry={loadChannelsConfig}
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
        {form.unavailable_enabled_channel ? (
          <InlineAlert
            message={t("config.unavailableEnabledChannel", {
              channel: form.unavailable_enabled_channel,
            })}
          />
        ) : null}
        <SettingsRow
          label={t("config.enabledChannel")}
          description={t("config.enabledChannelHelp")}
        >
          <TextField
            select
            hiddenLabel
            fullWidth
            value={selectedChannelValue}
            onChange={(e) =>
              update("enabled_channel", channelSelectValueToConfigValue(e.target.value))
            }
            aria-label={t("config.enabledChannel")}
            slotProps={{
              inputLabel: { shrink: true },
            }}
          >
            {form.available_channels.map((channelId) => (
              <MenuItem
                key={channelId || "disabled"}
                value={channelConfigValueToSelectValue(channelId)}
              >
                {t(enabledChannelLabelKey(channelId))}
              </MenuItem>
            ))}
          </TextField>
        </SettingsRow>
        {availableChannels.has("telegram") ? (
        <FormSectionSubCollapsible
          title="Telegram"
          defaultOpen={!form.enabled_channel || form.enabled_channel === "telegram"}
        >
          <FormGrid>
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
          </FormGrid>
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
        ) : null}

        {availableChannels.has("feishu") ? (
        <FormSectionSubCollapsible
          title={t("config.feishu")}
          defaultOpen={form.enabled_channel === "feishu"}
        >
          <FormGrid>
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
              label={t("config.feishuAllowedChatIds")}
              value={form.feishu_allowed_chat_ids}
              onChange={(e) => update("feishu_allowed_chat_ids", e.target.value)}
              fullWidth
              slotProps={{ htmlInput: { maxLength: MAX_LEN * 4 } }}
            />
          </FormGrid>
        </FormSectionSubCollapsible>
        ) : null}

        {availableChannels.has("dingtalk") ? (
        <FormSectionSubCollapsible
          title={t("config.dingtalk")}
          defaultOpen={form.enabled_channel === "dingtalk"}
        >
          <FormGrid>
            <TextField
              label={t("config.dingtalkClientId")}
              value={form.dingtalk_client_id}
              onChange={(e) => update("dingtalk_client_id", e.target.value)}
              fullWidth
              slotProps={{
                htmlInput: {
                  maxLength: MAX_LEN,
                  style: { fontFamily: "var(--font-mono)" },
                },
              }}
            />
            <TextField
              label={t("config.dingtalkClientSecret")}
              value={form.dingtalk_client_secret}
              onChange={(e) => update("dingtalk_client_secret", e.target.value)}
              type={isRevealed("dingtalk_client_secret") ? "text" : "password"}
              fullWidth
              slotProps={{
                htmlInput: {
                  maxLength: MAX_LEN,
                  ...getRevealHandlers("dingtalk_client_secret"),
                },
              }}
            />
          </FormGrid>
        </FormSectionSubCollapsible>
        ) : null}

        {availableChannels.has("wecom") ? (
        <FormSectionSubCollapsible
          title={t("config.wecom")}
          defaultOpen={form.enabled_channel === "wecom"}
        >
          <FormGrid>
            <TextField
              label={t("config.wecomBotId")}
              value={form.wecom_bot_id}
              onChange={(e) => update("wecom_bot_id", e.target.value)}
              fullWidth
              slotProps={{
                htmlInput: {
                  maxLength: MAX_LEN,
                  style: { fontFamily: "var(--font-mono)" },
                },
              }}
            />
            <TextField
              label={t("config.wecomBotSecret")}
              value={form.wecom_bot_secret}
              onChange={(e) => update("wecom_bot_secret", e.target.value)}
              type={isRevealed("wecom_bot_secret") ? "text" : "password"}
              fullWidth
              slotProps={{
                htmlInput: {
                  maxLength: MAX_LEN,
                  ...getRevealHandlers("wecom_bot_secret"),
                },
              }}
            />
            <TextField
              label={t("config.wecomWsUrl")}
              value={form.wecom_ws_url}
              onChange={(e) => update("wecom_ws_url", e.target.value)}
              type="url"
              fullWidth
              helperText={`${form.wecom_ws_url.length}/${MAX_URL}`}
              slotProps={{
                htmlInput: {
                  maxLength: MAX_URL,
                  style: { fontFamily: "var(--font-mono)" },
                },
              }}
            />
          </FormGrid>
        </FormSectionSubCollapsible>
        ) : null}

        {availableChannels.has("qq_channel") ? (
        <FormSectionSubCollapsible
          title={t("config.qqChannel")}
          defaultOpen={form.enabled_channel === "qq_channel"}
        >
          <FormGrid>
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
          </FormGrid>
        </FormSectionSubCollapsible>
        ) : null}

        <FormSectionSubCollapsible title="Webhook" defaultOpen={form.webhook_enabled}>
          <FormSwitchRow
            title={t("config.webhookEnabled")}
            divider={false}
            control={
              <Switch
                checked={form.webhook_enabled}
                onChange={(e) => update("webhook_enabled", e.target.checked)}
              />
            }
          />
          <FormGrid columns={1}>
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
          </FormGrid>
        </FormSectionSubCollapsible>
          </>
        )}
      </SettingsSection>
    </Box>
  );
}
