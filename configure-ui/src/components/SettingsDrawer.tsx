import Box from "@mui/material/Box";
import Drawer from "@mui/material/Drawer";
import FormControl from "@mui/material/FormControl";
import IconButton from "@mui/material/IconButton";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import Stack from "@mui/material/Stack";
import ToggleButton from "@mui/material/ToggleButton";
import ToggleButtonGroup from "@mui/material/ToggleButtonGroup";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import CloseRoundedIcon from "@mui/icons-material/CloseRounded";
import DarkModeOutlinedIcon from "@mui/icons-material/DarkModeOutlined";
import LanguageIcon from "@mui/icons-material/Language";
import PaletteOutlinedIcon from "@mui/icons-material/PaletteOutlined";
import SettingsRoundedIcon from "@mui/icons-material/SettingsRounded";
import WbSunnyOutlinedIcon from "@mui/icons-material/WbSunnyOutlined";
import type { SelectChangeEvent } from "@mui/material/Select";
import { useTranslation } from "react-i18next";
import { useAppPreferences } from "../hooks/useAppPreferences";
import {
  isLanguage,
  type AppLanguage,
} from "../contexts/appPreferencesContext";
import { THEME_BRAND_KEYS } from "../config/themeTokens";
import { SETTINGS_DRAWER_WIDTH } from "../config/layout";
import { PANEL_SECTION_PADDING } from "../theme/panelStyles";
import { SettingsSection } from "./SettingsSection";

interface SettingsDrawerProps {
  open: boolean;
  onClose: () => void;
}

export function SettingsDrawer({ open, onClose }: SettingsDrawerProps) {
  const { t } = useTranslation();
  const {
    language,
    setLanguage,
    themeMode,
    setThemeMode,
    themeBrand,
    setThemeBrand,
  } = useAppPreferences();

  const handleLanguageChange = (event: SelectChangeEvent<unknown>) => {
    const value = event.target.value;
    if (isLanguage(value)) setLanguage(value);
  };

  const handleThemeModeChange = (
    _: React.MouseEvent<HTMLElement>,
    value: "light" | "dark" | null,
  ) => {
    if (value !== null) setThemeMode(value);
  };

  return (
    <Drawer
      anchor="right"
      open={open}
      onClose={onClose}
      slotProps={{
        backdrop: {
          sx: { backgroundColor: "var(--backdrop-overlay)" },
        },
      }}
      sx={{
        "& .MuiDrawer-paper": {
          width: SETTINGS_DRAWER_WIDTH,
          maxWidth: "100%",
          boxSizing: "border-box",
          border: "none",
          borderLeft: "none",
          borderTopLeftRadius: "var(--radius-card)",
          borderBottomLeftRadius: "var(--radius-card)",
          boxShadow: "var(--shadow-shell-floating)",
          backgroundColor: "var(--card)",
          transition:
            "border-color var(--transition-duration) ease, box-shadow var(--transition-duration) ease",
        },
      }}
    >
      <Box
        sx={{
          height: "100%",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
        }}
      >
        <Stack
          direction="row"
          alignItems="center"
          justifyContent="space-between"
          sx={{ px: PANEL_SECTION_PADDING, py: 2, minHeight: 68 }}
        >
          <Stack direction="row" alignItems="center" spacing={1.5}>
            <Box
              sx={{
                width: "var(--icon-container-md)",
                height: "var(--icon-container-md)",
                borderRadius: "var(--radius-control)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                bgcolor: "var(--surface)",
                color: "var(--muted)",
                transition:
                  "background-color var(--transition-duration) ease, color var(--transition-duration) ease",
              }}
            >
              <SettingsRoundedIcon sx={{ fontSize: "var(--icon-size-md)" }} />
            </Box>
            <Typography
              variant="h6"
              sx={{
                fontWeight: 700,
                letterSpacing: "-0.01em",
                lineHeight: "var(--line-height-snug)",
              }}
            >
              {t("settings.title")}
            </Typography>
          </Stack>
          <IconButton
            size="small"
            onClick={onClose}
            aria-label={t("settings.close")}
            sx={{
              borderRadius: "var(--radius-chip)",
              color: "var(--muted)",
              transition: "color var(--transition-duration) ease",
              "&:hover": { color: "var(--foreground)" },
            }}
          >
            <CloseRoundedIcon fontSize="small" />
          </IconButton>
        </Stack>

        <Stack
          sx={{ flex: 1, overflow: "auto", p: PANEL_SECTION_PADDING }}
          spacing={1.5}
        >
          <SettingsSection
            icon={<LanguageIcon sx={{ fontSize: "var(--icon-size-sm)" }} />}
            label={t("settings.language")}
          >
            <FormControl fullWidth>
              <Select<AppLanguage>
                value={language}
                onChange={handleLanguageChange}
                displayEmpty
                inputProps={{ "aria-label": t("settings.language") }}
                renderValue={(v) =>
                  t(`settings.lang${v === "zh-CN" ? "Zh" : "En"}`)
                }
                sx={{
                  borderRadius: "var(--radius-control)",
                  "& .MuiOutlinedInput-notchedOutline": {
                    borderColor:
                      "color-mix(in srgb, var(--border) 60%, transparent)",
                  },
                }}
              >
                <MenuItem value="zh-CN">{t("settings.langZh")}</MenuItem>
                <MenuItem value="en-US">{t("settings.langEn")}</MenuItem>
              </Select>
            </FormControl>
          </SettingsSection>

          <SettingsSection
            icon={
              themeMode === "light" ? (
                <WbSunnyOutlinedIcon sx={{ fontSize: "var(--icon-size-sm)" }} />
              ) : (
                <DarkModeOutlinedIcon
                  sx={{ fontSize: "var(--icon-size-sm)" }}
                />
              )
            }
            label={t("settings.themeMode")}
          >
            <ToggleButtonGroup
              value={themeMode}
              exclusive
              onChange={handleThemeModeChange}
              fullWidth
            >
              <ToggleButton value="light">
                <WbSunnyOutlinedIcon
                  sx={{ fontSize: "var(--font-size-caption)", mr: 0.75 }}
                />
                {t("settings.light")}
              </ToggleButton>
              <ToggleButton value="dark">
                <DarkModeOutlinedIcon
                  sx={{ fontSize: "var(--font-size-caption)", mr: 0.75 }}
                />
                {t("settings.dark")}
              </ToggleButton>
            </ToggleButtonGroup>
          </SettingsSection>

          <SettingsSection
            icon={
              <PaletteOutlinedIcon sx={{ fontSize: "var(--icon-size-sm)" }} />
            }
            label={t("settings.themeBrand")}
            accessory={
              <Typography
                variant="caption"
                sx={{
                  fontSize: "var(--font-size-caption)",
                  color: "var(--muted)",
                  fontWeight: 600,
                  lineHeight: "var(--line-height-normal)",
                }}
              >
                {t(`settings.${themeBrand}`)}
              </Typography>
            }
          >
            <Stack direction="row" spacing={1.5} alignItems="center">
              {THEME_BRAND_KEYS.map((brand) => {
                const isSelected = themeBrand === brand;
                return (
                  <Tooltip key={brand} title={t(`settings.${brand}`)}>
                    <Box
                      component="button"
                      type="button"
                      aria-label={t(`settings.${brand}`)}
                      aria-pressed={isSelected}
                      onClick={() => setThemeBrand(brand)}
                      sx={{
                        width: "var(--icon-size-lg)",
                        height: "var(--icon-size-lg)",
                        borderRadius: "50%",
                        cursor: "pointer",
                        backgroundColor: `var(--brand-${brand})`,
                        border: "none",
                        padding: 0,
                        flexShrink: 0,
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "center",
                        transition:
                          "transform var(--transition-duration-emphasized) var(--ease-emphasized)",
                        outline: isSelected
                          ? "var(--focus-ring-width) solid var(--foreground)"
                          : "var(--focus-ring-width) solid transparent",
                        outlineOffset: isSelected
                          ? "var(--focus-ring-offset)"
                          : 0,
                        transform: isSelected ? "scale(1.06)" : "scale(1)",
                        "&:hover": {
                          transform: isSelected ? "scale(1.08)" : "scale(1.06)",
                        },
                        "&:focus-visible": {
                          outline:
                            "var(--focus-ring-width) solid var(--primary)",
                          outlineOffset: "var(--focus-ring-offset)",
                        },
                      }}
                    >
                      {isSelected && (
                        <Box
                          component="svg"
                          viewBox="0 0 14 14"
                          sx={{
                            width: 13,
                            height: 13,
                            fill: "none",
                            stroke: "var(--primary-fg)",
                            strokeWidth: 2.5,
                            strokeLinecap: "round",
                            strokeLinejoin: "round",
                            pointerEvents: "none",
                          }}
                        >
                          <path d="M2.5 7l3.5 3.5 5.5-6" />
                        </Box>
                      )}
                    </Box>
                  </Tooltip>
                );
              })}
            </Stack>
          </SettingsSection>
        </Stack>

        <Stack
          direction="row"
          alignItems="center"
          justifyContent="center"
          spacing={1.25}
          sx={{
            px: PANEL_SECTION_PADDING,
            py: 2,
            borderTop: "none",
          }}
        >
          <Box
            sx={{
              width: "var(--icon-size-sm)",
              height: "var(--icon-size-sm)",
              borderRadius: "var(--radius-chip)",
              background: "var(--surface)",
              border: "none",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
            }}
          >
            <Typography
              sx={{
                color: "var(--muted)",
                fontWeight: 700,
                fontSize: "var(--font-size-label)",
                letterSpacing: "-0.02em",
                lineHeight: "var(--line-height-tight)",
              }}
            >
              AI
            </Typography>
          </Box>
          <Typography
            variant="caption"
            sx={{
              fontSize: "var(--font-size-caption)",
              color: "var(--muted)",
              fontWeight: 500,
              lineHeight: "var(--line-height-normal)",
            }}
          >
            {t("app.name")}
          </Typography>
        </Stack>
      </Box>
    </Drawer>
  );
}
