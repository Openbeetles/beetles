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
import { UserFormBody } from "./formBodies";

/** 个性配置 → USER Tab（路由子页） */
export function SoulUserUserPanel() {
  const { t } = useTranslation();
  const {
    ready,
    retryLoadUser,
    userForm,
    setUserForm,
    userState,
    userSaveStatus,
    userError,
    handleSaveUser,
    dismissUserSaveFeedback,
  } = useSoulUserConfig();

  const userAlert = userState.error || null;
  const saveDisabled =
    !ready || userSaveStatus === "saving" || userState.loading;

  return (
    <Box sx={PAGE_STACK_OUTER_SX}>
      <InlineAlert message={userAlert} onRetry={retryLoadUser} />
      <SettingsSection
        pinHeader
        surfaceTone={userState.loading ? "loading" : "default"}
        sx={{ flex: 1, minHeight: 0 }}
        icon={<Os3dIcon src={OS_ICON_NAV["/soul-user"]} />}
        label={t("soulUser.sectionUser")}
        description={t("soulUser.userDesc")}
        accessory={
          <Button
            size="small"
            variant="contained"
            startIcon={<SaveRounded />}
            onClick={() => {
              void handleSaveUser();
            }}
            disabled={saveDisabled}
          >
            {userSaveStatus === "saving" ? t("common.saving") : t("common.save")}
          </Button>
        }
        belowTitleRow={
          userSaveStatus === "ok" || userSaveStatus === "fail" ? (
            <SaveFeedback
              placement="belowTitle"
              status={userSaveStatus}
              message={userSaveStatus === "ok" ? t("common.saveOk") : userError}
              autoDismissMs={3000}
              onDismiss={dismissUserSaveFeedback}
            />
          ) : null
        }
      >
        {userState.loading ? (
          <PanelStateLoading>
            <SectionLoadingSkeleton />
          </PanelStateLoading>
        ) : (
          <UserFormBody form={userForm} setForm={setUserForm} t={t} />
        )}
      </SettingsSection>
    </Box>
  );
}
