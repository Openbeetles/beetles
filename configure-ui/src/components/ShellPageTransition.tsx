import Box from "@mui/material/Box";
import { Outlet, useLocation } from "react-router-dom";

/**
 * 主工作区路由切换时的轻量入场动效（纯 CSS，无 motion 库）。
 * Lightweight page enter on route change; respects `prefers-reduced-motion`.
 */
export function ShellPageTransition() {
  const { pathname } = useLocation();
  return (
    <Box
      key={pathname}
      className="shell-page-enter"
      sx={{
        flex: 1,
        minHeight: 0,
        minWidth: 0,
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
        alignSelf: "stretch",
      }}
    >
      <Outlet />
    </Box>
  );
}
