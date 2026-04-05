/**
 * 主内容区在 `var(--surface)` 上叠 PCB 丝印：96px 板框参考格 + 正交细网格 + 双环焊盘 + 过孔 + 主色格点
 * + 疏对角走线 + 角对位点；金属高光慢移；矢量封装见 [components/PcbDecorOverlay.tsx]。
 * `prefers-reduced-motion: reduce` 下关闭动画。
 */
export const MAIN_SURFACE_PCB_SX = {
  "@keyframes pcbMainGridDrift": {
    "0%": {
      backgroundPosition:
        "0px 0px, 0px 0px, 0px 0px, 0px 0px, 0px 0px, 8px 8px, 0px 0px, 0px 0px, 12px 12px, 0% 0%",
    },
    "100%": {
      backgroundPosition:
        "4px 4px, 4px 4px, 32px 32px, 24px 24px, 32px 32px, 24px 24px, 128px 128px, 48px 48px, 44px 44px, 100% 100%",
    },
  },
  backgroundColor: "var(--surface)",
  backgroundImage: [
    // 大格对齐线（极淡，模拟板框/拼板参考）
    "repeating-linear-gradient(90deg, transparent 0, transparent 95px, color-mix(in srgb, var(--foreground) 2.2%, transparent) 95px, color-mix(in srgb, var(--foreground) 2.2%, transparent) 96px, transparent 96px)",
    "repeating-linear-gradient(0deg, transparent 0, transparent 95px, color-mix(in srgb, var(--foreground) 2.2%, transparent) 95px, color-mix(in srgb, var(--foreground) 2.2%, transparent) 96px, transparent 96px)",
    // 丝印竖线（中心略提亮，金属边缘感）
    "repeating-linear-gradient(90deg, color-mix(in srgb, var(--foreground) 5%, transparent) 0, color-mix(in srgb, white 12%, var(--foreground) 9%, transparent) 0.7px, color-mix(in srgb, var(--foreground) 6%, transparent) 1.4px, transparent 2px, transparent 32px)",
    // 丝印横线
    "repeating-linear-gradient(0deg, color-mix(in srgb, var(--foreground) 5%, transparent) 0, color-mix(in srgb, white 12%, var(--foreground) 9%, transparent) 0.7px, color-mix(in srgb, var(--foreground) 6%, transparent) 1.4px, transparent 2px, transparent 32px)",
    // 主焊盘（双环感：外圈 + 芯）
    "radial-gradient(circle, transparent 0.55px, color-mix(in srgb, var(--foreground) 11%, transparent) 0.95px, color-mix(in srgb, var(--foreground) 16%, transparent) 1.45px, transparent 2.15px)",
    // 细过孔 16px
    "radial-gradient(circle, color-mix(in srgb, var(--foreground) 7%, transparent) 0.85px, transparent 1.25px)",
    // 大格主色点缀（略柔边）
    "radial-gradient(circle at 50% 50%, color-mix(in srgb, var(--primary) 12%, transparent) 1.6px, color-mix(in srgb, var(--primary) 5%, transparent) 2.6px, transparent 5.5px)",
    // 对角走线（更疏，像松耦合总线）
    "repeating-linear-gradient(127deg, color-mix(in srgb, var(--foreground) 2.6%, transparent) 0 1px, transparent 1px 56px)",
    // 角部对位点（每 96px 单元一角，慢移）
    "radial-gradient(circle at 2px 2px, color-mix(in srgb, var(--primary) 9%, transparent) 1px, transparent 2.2px)",
    // 金属高光刷痕（慢移）
    "linear-gradient(118deg, transparent 0%, color-mix(in srgb, var(--foreground) 1.8%, transparent) 38%, color-mix(in srgb, white 20%, var(--foreground) 8%) 49.5%, color-mix(in srgb, var(--foreground) 2.2%, transparent) 61%, transparent 100%)",
  ].join(", "),
  backgroundSize:
    "96px 96px, 96px 96px, 32px 32px, 32px 32px, 32px 32px, 16px 16px, 128px 128px, 56px 56px, 96px 96px, 220% 220%",
  backgroundPosition:
    "0px 0px, 0px 0px, 0px 0px, 0px 0px, 0px 0px, 8px 8px, 0px 0px, 0px 0px, 12px 12px, 0% 0%",
  animation: "pcbMainGridDrift 40s linear infinite",
  "@media (prefers-reduced-motion: reduce)": {
    animation: "none",
  },
} as const;

/**
 * 侧栏选中项丝印：用绝对定位子节点绘制（不用 ::before，避免与 MUI ButtonBase/ListItem 伪元素冲突）。
 * 对比度刻意抬高，否则叠在主色浅底上几乎不可见。
 */
export function sidebarNavSelectedPcbOverlaySx() {
  return {
    backgroundImage: [
      "repeating-linear-gradient(90deg, color-mix(in srgb, var(--foreground) 22%, transparent) 0, color-mix(in srgb, white 14%, var(--foreground) 18%, transparent) 0.8px, color-mix(in srgb, var(--foreground) 14%, transparent) 1.45px, transparent 2px, transparent 24px)",
      "repeating-linear-gradient(0deg, color-mix(in srgb, var(--foreground) 22%, transparent) 0, color-mix(in srgb, white 14%, var(--foreground) 18%, transparent) 0.8px, color-mix(in srgb, var(--foreground) 14%, transparent) 1.45px, transparent 2px, transparent 24px)",
      "radial-gradient(circle, transparent 0.5px, color-mix(in srgb, var(--primary) 42%, transparent) 1.1px, color-mix(in srgb, var(--primary) 58%, transparent) 1.6px, transparent 2.35px)",
      "radial-gradient(circle, color-mix(in srgb, var(--foreground) 18%, transparent) 0.7px, transparent 1.1px)",
      "repeating-linear-gradient(127deg, color-mix(in srgb, var(--foreground) 12%, transparent) 0 1px, transparent 1px 36px)",
      "radial-gradient(circle at 1px 1px, color-mix(in srgb, var(--primary) 35%, transparent) 0.9px, transparent 1.8px)",
    ].join(", "),
    backgroundSize: "24px 24px, 24px 24px, 24px 24px, 12px 12px, 40px 40px, 48px 48px",
    backgroundPosition: "0 0, 0 0, 0 0, 6px 6px, 0 0, 4px 4px",
    backgroundRepeat: "repeat, repeat, repeat, repeat, repeat, repeat",
  } as const;
}
