import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import { ConfigSubNavLayout } from "../../components/ConfigSubNavLayout";
import { SoulUserConfigProvider } from "../../contexts/SoulUserConfigProvider";
import { PAGE_COLUMN_FILL_SX } from "../../theme/panelStyles";

function SoulUserLayoutShell() {
  const { t } = useTranslation();
  const subNavItems = [
    { segment: "soul", label: t("soulUser.tabSoul") },
    { segment: "user", label: t("soulUser.tabUser") },
  ];

  return (
    <Box sx={PAGE_COLUMN_FILL_SX}>
      <ConfigSubNavLayout basePath="/soul-user" items={subNavItems} />
    </Box>
  );
}

/**
 * 个性配置壳层：左侧分区导航 + 右侧子路由（与 `/device-config` 同构）。
 */
export function SoulUserLayout() {
  return (
    <SoulUserConfigProvider>
      <SoulUserLayoutShell />
    </SoulUserConfigProvider>
  );
}
