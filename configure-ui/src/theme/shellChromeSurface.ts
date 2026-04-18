/**
 * 壳层磨砂：半透明实体板，而不是网页玻璃卡。
 * 高光、接触影、轻微材料渐变都由统一 token 驱动，读起来更像桌面 OS chrome。
 */

/** 略提饱和，磨砂更「奶」、少数码感；不在此混品牌色，避免与中性 3D 阴影串色 */
const CHROME_BLUR =
  "saturate(1.02) blur(var(--shell-chrome-blur))";

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
  backgroundColor: "color-mix(in srgb, var(--surface) 84%, transparent)",
  backgroundImage: [
    "linear-gradient(180deg, color-mix(in srgb, #fff 16%, transparent) 0%, color-mix(in srgb, #fff 6%, transparent) 38%, transparent 82%)",
    "linear-gradient(180deg, color-mix(in srgb, var(--foreground) 1.2%, transparent) 0%, transparent 100%)",
  ].join(", "),
  ...chromeBackdrop,
} as const;

/** 顶栏：与主内容区分的哑光条 + OS3D 上沿高光（`--os3d-chrome-titlebar-stack`） */
export const SHELL_TITLEBAR_CHROME_SX = {
  ...SHELL_CHROME_SURFACE_SX,
  backgroundColor: "color-mix(in srgb, var(--surface) 82%, transparent)",
  boxShadow: "var(--os3d-chrome-titlebar-stack)",
} as const;

/** 任务栏：贴底哑光条 + 台面承托浅影（`--os3d-chrome-taskbar-stack`） */
export const SHELL_TASKBAR_CHROME_SX = {
  ...SHELL_CHROME_SURFACE_SX,
  backgroundColor: "color-mix(in srgb, var(--surface) 90%, transparent)",
  boxShadow: "var(--os3d-chrome-taskbar-stack)",
} as const;
