/**
 * 壳层磨砂：半透明 + backdrop blur，无背景渐变（扁平哑光）。
 * 立体层次由 `:root` 的 `--os3d-chrome-*-stack`（上沿 inset 高光 + 任务栏弱上影）提供，与 `os3dLanguage.ts` 一致。
 */

/** 略提饱和，磨砂更「奶」、少数码感；不在此混品牌色，避免与中性 3D 阴影串色 */
const CHROME_BLUR =
  "saturate(1.04) blur(var(--shell-chrome-blur))";

const chromeBackdrop = {
  backdropFilter: CHROME_BLUR,
  WebkitBackdropFilter: CHROME_BLUR,
  "@media (prefers-reduced-motion: reduce)": {
    backgroundColor: "var(--surface)",
    backgroundImage: "none",
    backdropFilter: "none",
    WebkitBackdropFilter: "none",
  },
} as const;

/** 通用壳层底（侧栏遗留场景、默认回退） */
export const SHELL_CHROME_SURFACE_SX = {
  backgroundColor: "color-mix(in srgb, var(--surface) 82%, transparent)",
  backgroundImage: "none",
  ...chromeBackdrop,
} as const;

/** 顶栏：与主内容区分的哑光条 + OS3D 上沿高光（`--os3d-chrome-titlebar-stack`） */
export const SHELL_TITLEBAR_CHROME_SX = {
  ...SHELL_CHROME_SURFACE_SX,
  backgroundColor: "color-mix(in srgb, var(--surface) 80%, transparent)",
  boxShadow: "var(--os3d-chrome-titlebar-stack)",
} as const;

/** 任务栏：贴底哑光条 + 台面承托浅影（`--os3d-chrome-taskbar-stack`） */
export const SHELL_TASKBAR_CHROME_SX = {
  ...SHELL_CHROME_SURFACE_SX,
  backgroundColor: "color-mix(in srgb, var(--surface) 83%, transparent)",
  boxShadow: "var(--os3d-chrome-taskbar-stack)",
} as const;
