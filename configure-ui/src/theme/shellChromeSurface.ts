/**
 * 壳层磨砂：半透明实体板，而不是网页玻璃卡。
 * 高光、接触影、轻微材料渐变都由统一 token 驱动，读起来更像桌面 OS chrome。
 */

/** 略提饱和，磨砂更「奶」、少数码感；不在此混品牌色，避免与中性 3D 阴影串色 */
const CHROME_BLUR =
  "saturate(1.08) blur(var(--shell-chrome-blur))";

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
  backgroundColor: "color-mix(in srgb, var(--surface) 68%, transparent)",
  backgroundImage: [
    "linear-gradient(180deg, color-mix(in srgb, #fff 12%, transparent) 0%, color-mix(in srgb, #fff 3%, transparent) 45%, transparent 100%)",
    "linear-gradient(90deg, color-mix(in srgb, var(--primary) 6%, transparent) 0%, transparent 22%, transparent 78%, color-mix(in srgb, var(--accent) 5%, transparent) 100%)",
  ].join(", "),
  ...chromeBackdrop,
} as const;

/** 顶栏：与主内容区分的半透条 + 玻璃感高光边 */
export const SHELL_TITLEBAR_CHROME_SX = {
  ...SHELL_CHROME_SURFACE_SX,
  backgroundColor: "color-mix(in srgb, var(--surface) 62%, transparent)",
  boxShadow: [
    "inset 0 1px 0 rgba(255, 255, 255, 0.55)",
    "inset 0 -1px 0 color-mix(in srgb, var(--border) 10%, transparent)",
  ].join(", "),
} as const;

/** 任务栏：贴底半透条 + 轻托起阴影 */
export const SHELL_TASKBAR_CHROME_SX = {
  ...SHELL_CHROME_SURFACE_SX,
  backgroundColor: "color-mix(in srgb, var(--surface) 70%, transparent)",
  boxShadow: [
    "inset 0 1px 0 rgba(255, 255, 255, 0.45)",
    "0 -4px 20px -8px color-mix(in srgb, var(--foreground) 6%, transparent)",
  ].join(", "),
} as const;
