/**
 * 壳层磨砂：半透明 + backdrop blur，无背景渐变（扁平哑光）。
 * Shell chrome: translucent blur, flat matte fill — no background gradients.
 */

const CHROME_BLUR =
  "saturate(1.1) blur(var(--shell-chrome-blur))";

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
  backgroundColor: "color-mix(in srgb, var(--surface) 78%, transparent)",
  backgroundImage: "none",
  ...chromeBackdrop,
} as const;

/** 顶栏：与主内容区分的哑光条 */
export const SHELL_TITLEBAR_CHROME_SX = {
  ...SHELL_CHROME_SURFACE_SX,
  backgroundColor: "color-mix(in srgb, var(--surface) 76%, transparent)",
} as const;

/** 任务栏：贴底哑光条 */
export const SHELL_TASKBAR_CHROME_SX = {
  ...SHELL_CHROME_SURFACE_SX,
  backgroundColor: "color-mix(in srgb, var(--surface) 80%, transparent)",
} as const;
