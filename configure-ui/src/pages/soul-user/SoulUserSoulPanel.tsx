import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import SaveRounded from "@mui/icons-material/SaveRounded";
import {
  InlineAlert,
  PanelStateLoading,
  SaveFeedback,
  SectionLoadingSkeleton,
} from "../../components/form";
import { Os3dIcon } from "../../components/Os3dIcon";
import { SettingsSection } from "../../components/SettingsSection";
import { OS_ICON_NAV } from "../../config/osIcons";
import { PAGE_STACK_OUTER_SX } from "../../theme/panelStyles";
import { useSoulUserConfig } from "../../hooks/useSoulUserConfig";
import { SoulFormBody } from "./formBodies";

/** 个性配置 → SOUL Tab（路由子页） */
export function SoulUserSoulPanel() {
  const { t } = useTranslation();
  const {
    ready,
    loadError,
    retryLoadSoul,
    soulForm,
    setSoulForm,
    soulState,
    soulSaveStatus,
    soulError,
    handleSaveSoul,
    dismissSoulSaveFeedback,
  } = useSoulUserConfig();

  const soulAlert = loadError || soulState.error || null;
  const saveDisabled =
    !ready || soulSaveStatus === "saving" || soulState.loading;

  return (
    <Box sx={PAGE_STACK_OUTER_SX}>
      <InlineAlert message={soulAlert} onRetry={retryLoadSoul} />
      <SettingsSection
        pinHeader
        surfaceTone={soulState.loading ? "loading" : "default"}
        sx={{ flex: 1, minHeight: 0 }}
        icon={<Os3dIcon src={OS_ICON_NAV["/soul-user"]} />}
        label={t("soulUser.sectionSoul")}
        description={t("soulUser.soulDesc")}
        accessory={
          <Button
            size="small"
            variant="contained"
            startIcon={<SaveRounded />}
            onClick={() => {
              void handleSaveSoul();
            }}
            disabled={saveDisabled}
          >
            {soulSaveStatus === "saving" ? t("common.saving") : t("common.save")}
          </Button>
        }
        belowTitleRow={
          soulSaveStatus === "ok" || soulSaveStatus === "fail" ? (
            <SaveFeedback
              placement="belowTitle"
              status={soulSaveStatus}
              message={soulSaveStatus === "ok" ? t("common.saveOk") : soulError}
              autoDismissMs={3000}
              onDismiss={dismissSoulSaveFeedback}
            />
          ) : null
        }
      >
        {soulState.loading ? (
          <PanelStateLoading>
            <SectionLoadingSkeleton />
          </PanelStateLoading>
        ) : (
          <SoulFormBody form={soulForm} setForm={setSoulForm} t={t} />
        )}
      </SettingsSection>
    </Box>
  );
}
