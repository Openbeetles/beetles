import { useEffect, useMemo, type PropsWithChildren } from "react";
import Box from "@mui/material/Box";
import CssBaseline from "@mui/material/CssBaseline";
import { ThemeProvider } from "@mui/material/styles";
import { createAppTheme } from "../theme/appTheme";
import { BEETLE_AMBIENT_BACKDROP_SX } from "../theme/beetleAmbientBackdrop";
import { useAppPreferences } from "../hooks/useAppPreferences";
import i18n from "../i18n";

/** 主色渐变：极淡；极慢位移让全局底有轻微「呼吸」感（与 main 区 PCB 漂移呼应） */
const gradientBackground = {
  "@keyframes pcbAmbientDrift": {
    "0%": { backgroundPosition: "0% 0%, 0% 0%, 0% 0%" },
    "50%": { backgroundPosition: "2% 1.5%, -1.5% 2%, 1% -1%" },
    "100%": { backgroundPosition: "0% 0%, 0% 0%, 0% 0%" },
  },
  position: "fixed" as const,
  inset: 0,
  zIndex: 0,
  pointerEvents: "none" as const,
  backgroundColor: "var(--background)",
  backgroundImage: [
    "linear-gradient(225deg, color-mix(in srgb, var(--primary) 12%, transparent) 0%, color-mix(in srgb, var(--primary) 5%, transparent) 42%, transparent 80%)",
    "radial-gradient(ellipse 95% 75% at 108% -15%, color-mix(in srgb, var(--primary) 9%, transparent), transparent 55%)",
    "radial-gradient(ellipse 65% 55% at 98% 98%, color-mix(in srgb, var(--accent) 8%, transparent), transparent 52%)",
  ].join(", "),
  backgroundSize: "100% 100%, 100% 100%, 100% 100%",
  backgroundPosition: "0 0, 0 0, 0 0",
  backgroundRepeat: "no-repeat",
  animation: "pcbAmbientDrift 48s ease-in-out infinite",
  "@media (prefers-reduced-motion: reduce)": {
    animation: "none",
  },
} as const;

/** 主内容区 PCB 点阵见 [theme/pcbSurface.ts]；全局甲壳虫氛围见 [theme/beetleAmbientBackdrop.ts]。 */

export function ThemeAndBaseline({ children }: PropsWithChildren) {
  const { language, themeMode, themeBrand } = useAppPreferences();
  const theme = useMemo(
    () => createAppTheme(themeMode, themeBrand),
    [themeMode, themeBrand],
  );

  useEffect(() => {
    void i18n.changeLanguage(language);
  }, [language]);

  useEffect(() => {
    document.documentElement.setAttribute("data-theme-mode", themeMode);
    document.documentElement.setAttribute("data-theme-brand", themeBrand);
  }, [themeMode, themeBrand]);

  return (
    <ThemeProvider theme={theme}>
      <CssBaseline />
      <Box aria-hidden sx={gradientBackground} />
      <Box aria-hidden sx={BEETLE_AMBIENT_BACKDROP_SX} />
      <Box sx={{ position: "relative", zIndex: 1 }}>{children}</Box>
    </ThemeProvider>
  );
}
