import { useEffect, useMemo, type PropsWithChildren } from "react";
import Box from "@mui/material/Box";
import CssBaseline from "@mui/material/CssBaseline";
import { ThemeProvider } from "@mui/material/styles";
import { createAppTheme } from "../theme/appTheme";
import { useAppPreferences } from "../hooks/useAppPreferences";
import i18n from "../i18n";

/** 全局底：由 `MuiCssBaseline` 的 `body { backgroundColor: var(--background) }` 提供纯色。 */

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
      <Box sx={{ position: "relative", zIndex: 1 }}>{children}</Box>
    </ThemeProvider>
  );
}
