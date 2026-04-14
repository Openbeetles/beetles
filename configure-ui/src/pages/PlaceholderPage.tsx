import { useTranslation } from "react-i18next";
import Typography from "@mui/material/Typography";
import Box from "@mui/material/Box";
import { PAGE_SCROLL_CANVAS_SX } from "../theme/panelStyles";

export function PlaceholderPage() {
  const { t } = useTranslation();
  return (
    <Box
      data-app-scroll-region
      sx={{
        ...PAGE_SCROLL_CANVAS_SX,
        alignItems: "center",
        justifyContent: "center",
        py: 8,
        textAlign: "center",
      }}
    >
      <Typography
        sx={{
          color: "var(--text-tertiary)",
          fontSize: "var(--font-size-body)",
          lineHeight: "var(--line-height-relaxed)",
        }}
      >
        {t("common.pageComingSoon")}
      </Typography>
    </Box>
  );
}
