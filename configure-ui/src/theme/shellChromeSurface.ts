/**
 * 侧栏 / 顶栏等壳层：半透明 surface + backdrop blur，使 fixed 全局渐变与甲壳虫氛围层可透出。
 * Shell chrome: translucent surface + blur so global backdrop (gradient + beetle ambient) reads through.
 */
export const SHELL_CHROME_SURFACE_SX = {
  backgroundColor: "color-mix(in srgb, var(--surface) 82%, transparent)",
  backdropFilter: "saturate(1.06) blur(var(--overlay-backdrop-blur))",
  WebkitBackdropFilter: "saturate(1.06) blur(var(--overlay-backdrop-blur))",
  "@media (prefers-reduced-motion: reduce)": {
    backgroundColor: "var(--surface)",
    backdropFilter: "none",
    WebkitBackdropFilter: "none",
  },
} as const;
