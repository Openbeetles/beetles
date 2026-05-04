import IconButton from "@mui/material/IconButton";
import Tooltip from "@mui/material/Tooltip";
import { useTranslation } from "react-i18next";
import { OS_ICON_NAV } from "../config/osIcons";
import { Os3dIcon } from "./Os3dIcon";

export function FloatingChatButton({ onClick }: { onClick: () => void }) {
  const { t } = useTranslation();
  const label = t("chat.open");

  return (
    <Tooltip title={label} placement="left">
      <IconButton
        aria-label={label}
        onClick={onClick}
        sx={{
          position: "fixed",
          right: {
            xs: "calc(16px + env(safe-area-inset-right))",
            sm: "calc(24px + env(safe-area-inset-right))",
          },
          bottom: {
            xs: "calc(82px + env(safe-area-inset-bottom))",
            sm: "calc(90px + env(safe-area-inset-bottom))",
          },
          zIndex: 1050,
          width: { xs: 58, sm: 66 },
          height: { xs: 58, sm: 66 },
          borderRadius: "50%",
          color: "var(--primary-fg)",
          backgroundColor:
            "color-mix(in srgb, var(--primary) 82%, var(--card))",
          backgroundImage: [
            "radial-gradient(circle at 32% 18%, color-mix(in srgb, #fff 70%, transparent) 0%, transparent 29%)",
            "linear-gradient(145deg, color-mix(in srgb, #fff 28%, transparent) 0%, transparent 44%, color-mix(in srgb, #000 14%, transparent) 100%)",
          ].join(", "),
          border:
            "1px solid color-mix(in srgb, var(--primary-fg) 34%, transparent)",
          boxShadow: [
            "inset 0 1px 0 color-mix(in srgb, #fff 72%, transparent)",
            "inset 0 -8px 14px -10px color-mix(in srgb, #000 26%, transparent)",
            "0 22px 36px -24px color-mix(in srgb, var(--primary) 46%, transparent)",
            "0 10px 18px -12px color-mix(in srgb, var(--foreground) 16%, transparent)",
          ].join(", "),
          transform: "translateZ(0)",
          transition:
            "transform var(--transition-duration-emphasized) var(--ease-emphasized), box-shadow var(--transition-duration-emphasized) var(--ease-emphasized), filter var(--transition-duration) var(--ease-out-smooth)",
          "&::after": {
            content: '""',
            position: "absolute",
            right: { xs: 8, sm: 9 },
            bottom: { xs: 5, sm: 6 },
            width: { xs: 14, sm: 16 },
            height: { xs: 14, sm: 16 },
            borderRadius: "4px 4px 12px 4px",
            backgroundColor:
              "color-mix(in srgb, var(--primary) 82%, var(--card))",
            transform: "rotate(28deg) skewX(-10deg)",
            boxShadow:
              "inset -1px -1px 0 color-mix(in srgb, #000 12%, transparent)",
          },
          "&:hover": {
            backgroundColor:
              "color-mix(in srgb, var(--primary) 88%, var(--card))",
            transform: "translateY(-3px)",
            boxShadow: [
              "inset 0 1px 0 color-mix(in srgb, #fff 78%, transparent)",
              "inset 0 -9px 15px -10px color-mix(in srgb, #000 28%, transparent)",
              "0 28px 42px -24px color-mix(in srgb, var(--primary) 54%, transparent)",
              "0 14px 22px -12px color-mix(in srgb, var(--foreground) 18%, transparent)",
            ].join(", "),
          },
          "&:focus-visible": {
            outline: "var(--focus-ring-width) solid var(--primary)",
            outlineOffset: "var(--focus-ring-offset)",
          },
          "@media (prefers-reduced-motion: reduce)": {
            transition: "none",
            "&:hover": {
              transform: "none",
            },
          },
        }}
      >
        <Os3dIcon
          src={OS_ICON_NAV["/channels-config"]}
          alt={label}
          decorative={false}
          variant="dock"
          sx={{
            position: "relative",
            zIndex: 1,
            width: { xs: 36, sm: 42 },
            height: { xs: 36, sm: 42 },
            transform: "translateY(-1px)",
          }}
        />
      </IconButton>
    </Tooltip>
  );
}
