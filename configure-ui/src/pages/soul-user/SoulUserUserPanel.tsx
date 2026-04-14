import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import SaveRounded from "@mui/icons-material/SaveRounded";
import PersonOutlined from "@mui/icons-material/PersonOutlined";
import {
  InlineAlert,
  SaveFeedback,
  SectionLoadingSkeleton,
} from "../../components/form";
import { SettingsSection } from "../../components/SettingsSection";
import { PAGE_COLUMN_FILL_SX } from "../../theme/panelStyles";
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
    <Box sx={{ ...PAGE_COLUMN_FILL_SX, gap: 2 }}>
      <InlineAlert message={userAlert} onRetry={retryLoadUser} />
      <SettingsSection
        pinHeader
        sx={{ flex: 1, minHeight: 0 }}
        icon={<PersonOutlined sx={{ fontSize: "var(--icon-size-md)" }} />}
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
          <SectionLoadingSkeleton />
        ) : (
          <UserFormBody form={userForm} setForm={setUserForm} t={t} />
        )}
      </SettingsSection>
    </Box>
  );
}
