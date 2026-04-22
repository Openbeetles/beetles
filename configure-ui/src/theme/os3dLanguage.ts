import type { ThemeMode } from "../config/themeTokens";

/**
 * Beetle OS「拟物 3D」单源：与官网 OsPanel 一致 — **实体板靠多层外扩漫反射 + 顶缘蜡光** 塑形，
 * 不靠细描边；凹格用 inset 影。配色仅用 foreground / 中性黑混色。
 */

export type Os3dIconVariant =
  | "default"
  /** 任务栏 Dock：已有按钮抬升，阴影更收、更柔 */
  | "dock"
  /** 开始菜单磁贴：环境光略强仍保持细腻 */
  | "tile"
  /** 大插画井 / Hero */
  | "hero"
  /** 顶栏小槽位、行内列表 */
  | "inline"
  /** 桌面标题栏：极浅接触影，避免图标在 caption rail 上视觉下坠 */
  | "titlebar";

/** CSS 变量名 → 供 `Os3dIcon` 与文档引用 */
export const OS3D_ICON_FILTER_VAR: Record<Os3dIconVariant, string> = {
  default: "var(--os3d-icon-filter-default)",
  dock: "var(--os3d-icon-filter-dock)",
  tile: "var(--os3d-icon-filter-tile)",
  hero: "var(--os3d-icon-filter-hero)",
  inline: "var(--os3d-icon-filter-inline)",
  titlebar: "var(--os3d-icon-filter-titlebar)",
};

export function os3dRootCssVars(mode: ThemeMode): Record<string, string> {
  const dark = mode === "dark";

  /**
   * 图标：三层柔光叠化 — 远距弥散 + 中距塑形 + 贴底接触线；半径偏大、alpha 偏低。
   */
  const iconDefault = dark
    ? [
        "drop-shadow(0 7px 16px rgba(0,0,0,0.17))",
        "drop-shadow(0 2px 5px rgba(0,0,0,0.09))",
      ].join(" ")
    : [
        "drop-shadow(0 12px 24px color-mix(in srgb, var(--foreground) 5.4%, transparent))",
        "drop-shadow(0 4px 9px color-mix(in srgb, var(--foreground) 2.6%, transparent))",
      ].join(" ");

  const iconDock = dark
    ? [
        "drop-shadow(0 3px 7px rgba(0,0,0,0.12))",
        "drop-shadow(0 1px 1px rgba(0,0,0,0.06))",
      ].join(" ")
    : [
        "drop-shadow(0 6px 12px color-mix(in srgb, var(--foreground) 3.4%, transparent))",
        "drop-shadow(0 2px 4px color-mix(in srgb, var(--foreground) 1.8%, transparent))",
      ].join(" ");

  const iconTile = dark
    ? [
        "drop-shadow(0 9px 20px rgba(0,0,0,0.2))",
        "drop-shadow(0 3px 7px rgba(0,0,0,0.11))",
      ].join(" ")
    : [
        "drop-shadow(0 14px 28px color-mix(in srgb, var(--foreground) 6.2%, transparent))",
        "drop-shadow(0 5px 10px color-mix(in srgb, var(--foreground) 3%, transparent))",
      ].join(" ");

  const iconHero = dark
    ? [
        "drop-shadow(0 12px 30px rgba(0,0,0,0.23))",
        "drop-shadow(0 4px 10px rgba(0,0,0,0.13))",
      ].join(" ")
    : [
        "drop-shadow(0 20px 40px color-mix(in srgb, var(--foreground) 7.2%, transparent))",
        "drop-shadow(0 6px 14px color-mix(in srgb, var(--foreground) 3.4%, transparent))",
      ].join(" ");

  const iconInline = dark
    ? "drop-shadow(0 3px 8px rgba(0,0,0,0.14)) drop-shadow(0 1px 2px rgba(0,0,0,0.08))"
    : [
        "drop-shadow(0 6px 12px color-mix(in srgb, var(--foreground) 3.8%, transparent))",
        "drop-shadow(0 1px 3px color-mix(in srgb, var(--foreground) 2.2%, transparent))",
      ].join(" ");

  const iconTitlebar = dark
    ? "drop-shadow(0 1px 1px rgba(0,0,0,0.09))"
    : "drop-shadow(0 1px 2px color-mix(in srgb, var(--foreground) 1.8%, transparent))";

  /**
   * 内容大板（Settings 左栏、仪表盘卡外壳）：远距体积光 + 中距承托 + 贴底接触 + 顶缘蜡光。
   * Content plate: ambient halo + layered lift (beetle_site marketing OsPanel, neutral-only).
   */
  const plateDark = [
    "0 20px 46px -30px color-mix(in srgb, #000 38%, transparent)",
    "0 8px 18px -12px color-mix(in srgb, #000 22%, transparent)",
    "inset 0 1px 0 color-mix(in srgb, var(--foreground) 8%, transparent)",
    "inset 0 12px 20px -20px color-mix(in srgb, var(--foreground) 4%, transparent)",
    "inset 0 -1px 0 color-mix(in srgb, #000 12%, transparent)",
  ].join(", ");

  const plateLight = [
    "0 26px 54px -38px color-mix(in srgb, var(--foreground) 8.6%, transparent)",
    "0 10px 20px -14px color-mix(in srgb, var(--foreground) 4.8%, transparent)",
    "0 2px 6px -3px color-mix(in srgb, var(--foreground) 2.4%, transparent)",
    "inset 0 1px 0 color-mix(in srgb, #fff 92%, transparent)",
    "inset 0 16px 24px -24px color-mix(in srgb, #fff 52%, transparent)",
    "inset 0 -1px 0 color-mix(in srgb, var(--foreground) 1.5%, transparent)",
  ].join(", ");

  const controlSoftLift = dark
    ? [
        "inset 0 1px 0 color-mix(in srgb, var(--foreground) 8%, transparent)",
        "0 5px 12px -10px color-mix(in srgb, #000 20%, transparent)",
        "0 1px 3px -2px color-mix(in srgb, #000 14%, transparent)",
      ].join(", ")
    : [
        "inset 0 1px 0 color-mix(in srgb, #fff 82%, transparent)",
        "inset 0 10px 18px -18px color-mix(in srgb, #fff 40%, transparent)",
        "0 9px 16px -12px color-mix(in srgb, var(--foreground) 5.4%, transparent)",
        "0 2px 5px -4px color-mix(in srgb, var(--foreground) 3%, transparent)",
      ].join(", ");

  const selectionPill = dark
    ? [
        "inset 0 1px 0 color-mix(in srgb, var(--foreground) 10%, transparent)",
        "0 7px 16px -11px color-mix(in srgb, #000 22%, transparent)",
        "0 3px 7px -4px color-mix(in srgb, var(--primary) 12%, transparent)",
      ].join(", ")
    : [
        "inset 0 1px 0 color-mix(in srgb, #fff 90%, transparent)",
        "inset 0 12px 20px -20px color-mix(in srgb, #fff 38%, transparent)",
        "0 10px 18px -14px color-mix(in srgb, var(--primary) 14%, transparent)",
        "0 3px 8px -6px color-mix(in srgb, var(--foreground) 4%, transparent)",
      ].join(", ");

  const sectionModule = dark
    ? [
        "inset 0 1px 0 color-mix(in srgb, var(--foreground) 6%, transparent)",
        "inset 0 12px 20px -22px color-mix(in srgb, var(--foreground) 3%, transparent)",
        "inset 0 -1px 0 color-mix(in srgb, #000 12%, transparent)",
        "0 10px 22px -18px color-mix(in srgb, #000 22%, transparent)",
      ].join(", ")
    : [
        "inset 0 1px 0 color-mix(in srgb, #fff 86%, transparent)",
        "inset 0 16px 24px -24px color-mix(in srgb, #fff 46%, transparent)",
        "inset 0 -1px 0 color-mix(in srgb, var(--foreground) 1.8%, transparent)",
        "0 14px 28px -22px color-mix(in srgb, var(--foreground) 5.5%, transparent)",
        "0 2px 6px -5px color-mix(in srgb, var(--foreground) 2.6%, transparent)",
      ].join(", ");

  const sectionModuleHeader = dark
    ? [
        "inset 0 1px 0 color-mix(in srgb, var(--foreground) 5%, transparent)",
        "inset 0 12px 18px -18px color-mix(in srgb, var(--foreground) 4%, transparent)",
        "inset 0 -1px 0 color-mix(in srgb, #000 14%, transparent)",
      ].join(", ")
    : [
        "inset 0 1px 0 color-mix(in srgb, #fff 56%, transparent)",
        "inset 0 12px 18px -18px color-mix(in srgb, #fff 34%, transparent)",
        "inset 0 -1px 0 color-mix(in srgb, var(--foreground) 2.2%, transparent)",
      ].join(", ");

  return {
    "--os3d-icon-filter-default": iconDefault,
    "--os3d-icon-filter-dock": iconDock,
    "--os3d-icon-filter-tile": iconTile,
    "--os3d-icon-filter-hero": iconHero,
    "--os3d-icon-filter-inline": iconInline,
    "--os3d-icon-filter-titlebar": iconTitlebar,

    /** 顶栏：统一主光源的薄高光 + 底缘接触影，读起来像实体窗口 chrome。 */
    "--os3d-chrome-titlebar-stack": [
      "inset 0 1px 0 color-mix(in srgb, #fff 58%, transparent)",
      "inset 0 12px 18px -18px color-mix(in srgb, #fff 18%, transparent)",
      "inset 0 -1px 0 color-mix(in srgb, var(--foreground) 2.4%, transparent)",
    ].join(", "),

    /** 任务栏：比顶栏更沉一点，像桌面底部承托出来的一块实体条。 */
    "--os3d-chrome-taskbar-stack": [
      "inset 0 1px 0 color-mix(in srgb, #fff 56%, transparent)",
      "inset 0 14px 20px -20px color-mix(in srgb, #fff 16%, transparent)",
      "inset 0 -1px 0 color-mix(in srgb, var(--foreground) 2.8%, transparent)",
      "0 -14px 30px -22px color-mix(in srgb, var(--foreground) 5.4%, transparent)",
      "0 -4px 10px -8px color-mix(in srgb, var(--foreground) 3.4%, transparent)",
    ].join(", "),

    "--shell-main-inset-top": [
      "inset 0 1px 0 color-mix(in srgb, #fff 52%, transparent)",
      "inset 0 16px 28px -28px color-mix(in srgb, var(--foreground) 10%, transparent)",
    ].join(", "),

    /** 插画井：更浅的碟形，底缘几乎只作分隔暗示 */
    "--os3d-icon-well-dish": [
      "inset 0 1px 0 color-mix(in srgb, var(--foreground) 4%, transparent)",
      "inset 0 10px 16px -16px color-mix(in srgb, #fff 20%, transparent)",
      "inset 0 -1px 0 color-mix(in srgb, var(--foreground) 1.8%, transparent)",
    ].join(", "),

    "--os3d-content-plate-stack": dark ? plateDark : plateLight,
    "--os3d-control-soft-lift-stack": controlSoftLift,
    "--os3d-selection-pill-stack": selectionPill,
    "--os3d-section-module-stack": sectionModule,
    "--os3d-section-module-header-stack": sectionModuleHeader,

    /**
     * 开始菜单 / Launch panel：比普通内容板更悬浮，远距阴影更大，
     * 底边接触影更集中，用来读出「离开桌面」的体积感。
     */
    "--os3d-start-panel-stack": dark
      ? [
          "0 28px 58px -30px color-mix(in srgb, #000 42%, transparent)",
          "0 12px 24px -14px color-mix(in srgb, #000 24%, transparent)",
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 10%, transparent)",
          "inset 0 12px 20px -20px color-mix(in srgb, var(--foreground) 5%, transparent)",
          "inset 0 -1px 0 color-mix(in srgb, #000 12%, transparent)",
        ].join(", ")
      : [
          "0 32px 62px -38px color-mix(in srgb, var(--foreground) 10%, transparent)",
          "0 13px 24px -16px color-mix(in srgb, var(--foreground) 6%, transparent)",
          "0 4px 10px -8px color-mix(in srgb, var(--foreground) 3.2%, transparent)",
          "inset 0 1px 0 color-mix(in srgb, #fff 94%, transparent)",
          "inset 0 16px 24px -24px color-mix(in srgb, #fff 50%, transparent)",
          "inset 0 -1px 0 color-mix(in srgb, var(--foreground) 1.8%, transparent)",
        ].join(", "),

    /**
     * 仪表盘卡顶栏：上缘蜡光 + 与正文分界用 **内阴影**（无 hairline border）。
     */
    "--os3d-dashboard-card-header-lip": [
      "inset 0 1px 0 color-mix(in srgb, #fff 46%, transparent)",
      "inset 0 12px 18px -18px color-mix(in srgb, #fff 18%, transparent)",
      "inset 0 -1px 0 color-mix(in srgb, var(--foreground) 1.8%, transparent)",
    ].join(", "),

    /**
     * 仪表盘卡正文相对标题栏：浅「屏坑」承托内容（标题栏略抬、正文略沉）。
     */
    "--os3d-dashboard-body-recess": dark
      ? [
          "inset 0 3px 10px color-mix(in srgb, #000 14%, transparent)",
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 3%, transparent)",
        ].join(", ")
      : [
          "inset 0 4px 14px color-mix(in srgb, var(--foreground) 4%, transparent)",
          "inset 0 1px 0 color-mix(in srgb, #fff 28%, transparent)",
        ].join(", "),

    /**
     * 配置子导航列表区：落在左栏大板内的浅槽（左栏仍用 content-plate 抬起）。
     */
    "--os3d-subnav-track-recess": dark
      ? [
          "inset 0 2px 6px color-mix(in srgb, #000 8%, transparent)",
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 2.4%, transparent)",
        ].join(", ")
      : [
          "inset 0 4px 10px color-mix(in srgb, var(--foreground) 3.4%, transparent)",
          "inset 0 1px 0 color-mix(in srgb, #fff 28%, transparent)",
        ].join(", "),

    "--os3d-banner-ribbon-stack": [
      "inset 0 1px 0 color-mix(in srgb, var(--foreground) 4.2%, transparent)",
      "0 6px 18px -10px color-mix(in srgb, var(--foreground) 3.8%, transparent)",
    ].join(", "),

    /** 语义条：以「浮起丝带」为主（外影略强），顶缘仅薄蜡光，不全凹 */
    "--os3d-alert-strip-stack": dark
      ? [
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 4%, transparent)",
          "0 4px 14px -6px color-mix(in srgb, var(--foreground) 8%, transparent)",
          "0 1px 3px -1px color-mix(in srgb, #000 10%, transparent)",
        ].join(", ")
      : [
          "inset 0 1px 0 color-mix(in srgb, #fff 22%, transparent)",
          "0 8px 18px -12px color-mix(in srgb, var(--foreground) 4.2%, transparent)",
          "0 2px 4px -3px color-mix(in srgb, var(--foreground) 2%, transparent)",
        ].join(", "),

    /**
     * 压在凹底/卡面上的「贴件」凸起：仪表盘数字块、通道 StatPill 等。
     * Raised chip: top wax + ambient below — reads above inset panels.
     */
    "--os3d-chip-lift-stack": dark
      ? [
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 10%, transparent)",
          "0 8px 16px -12px color-mix(in srgb, #000 24%, transparent)",
          "0 1px 3px -2px color-mix(in srgb, #000 14%, transparent)",
        ].join(", ")
      : [
          "inset 0 1px 0 color-mix(in srgb, #fff 78%, transparent)",
          "inset 0 10px 16px -16px color-mix(in srgb, #fff 26%, transparent)",
          "0 12px 22px -18px color-mix(in srgb, var(--foreground) 7%, transparent)",
          "0 2px 5px -4px color-mix(in srgb, var(--foreground) 3.2%, transparent)",
        ].join(", "),

    /**
     * 设置区标题旁图标槽、空态插画井：比 chip 更轻的承托凸起（小台座）。
     */
    "--os3d-pedestal-lift-stack": dark
      ? [
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 6%, transparent)",
          "0 6px 12px -10px color-mix(in srgb, #000 20%, transparent)",
        ].join(", ")
      : [
          "inset 0 1px 0 color-mix(in srgb, #fff 78%, transparent)",
          "inset 0 10px 14px -14px color-mix(in srgb, #fff 20%, transparent)",
          "0 10px 18px -14px color-mix(in srgb, var(--foreground) 5.5%, transparent)",
        ].join(", "),

    /** 顶栏面包屑胶囊：轻浮于磨砂条之上 */
    "--os3d-breadcrumb-lift-stack": [
      "inset 0 1px 0 color-mix(in srgb, var(--foreground) 2.4%, transparent)",
      "inset 0 10px 14px -14px color-mix(in srgb, #fff 18%, transparent)",
      "inset 0 -1px 0 color-mix(in srgb, var(--foreground) 0.8%, transparent)",
      "0 4px 10px -8px color-mix(in srgb, var(--foreground) 2.8%, transparent)",
    ].join(", "),

    /** 卡内嵌套：凹影更浅、边更柔（仅靠 inset，无 hairline） */
    "--os3d-inset-panel-stack": dark
      ? [
          "inset 0 3px 9px color-mix(in srgb, #000 14%, transparent)",
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 3%, transparent)",
        ].join(", ")
      : [
          "inset 0 4px 12px color-mix(in srgb, var(--foreground) 2.8%, transparent)",
          "inset 0 1px 0 color-mix(in srgb, #fff 20%, transparent)",
        ].join(", "),

    /**
     * 真·凹格（极少用）：仅适合需「按下/沉孔」语义的槽位；一般控件优先 `chip-lift`。
     */
    "--os3d-micro-well-stack": dark
      ? [
          "inset 0 3px 7px color-mix(in srgb, #000 18%, transparent)",
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 3%, transparent)",
        ].join(", ")
      : [
          "inset 0 4px 10px color-mix(in srgb, var(--foreground) 4.5%, transparent)",
          "inset 0 1px 0 color-mix(in srgb, #fff 24%, transparent)",
        ].join(", "),

    /**
     * 主色填充按钮（全站 `contained` + primary）：顶缘蜡光 + 色相贴合的外扩影；hover 略抬、active 内收。
     * Primary filled button: wax lip + tinted lift stack; hover lifts, active presses.
     */
    "--os3d-primary-button-stack": dark
      ? [
          "inset 0 1px 0 color-mix(in srgb, var(--primary-fg) 12%, transparent)",
          "0 4px 14px -6px color-mix(in srgb, #000 30%, transparent)",
          "0 1px 3px -1px color-mix(in srgb, #000 18%, transparent)",
        ].join(", ")
      : [
          "inset 0 1px 0 color-mix(in srgb, var(--primary-fg) 14%, transparent)",
          "inset 0 16px 20px -20px color-mix(in srgb, var(--primary-fg) 8%, transparent)",
          "0 10px 20px -14px color-mix(in srgb, var(--primary) 16%, transparent)",
          "0 2px 6px -4px color-mix(in srgb, var(--foreground) 3.8%, transparent)",
        ].join(", "),

    "--os3d-primary-button-stack-hover": dark
      ? [
          "inset 0 1px 0 color-mix(in srgb, var(--primary-fg) 16%, transparent)",
          "0 6px 18px -6px color-mix(in srgb, #000 34%, transparent)",
          "0 2px 5px -2px color-mix(in srgb, #000 22%, transparent)",
        ].join(", ")
      : [
          "inset 0 1px 0 color-mix(in srgb, var(--primary-fg) 16%, transparent)",
          "inset 0 18px 22px -22px color-mix(in srgb, var(--primary-fg) 10%, transparent)",
          "0 12px 22px -14px color-mix(in srgb, var(--primary) 18%, transparent)",
          "0 3px 7px -5px color-mix(in srgb, var(--foreground) 4.4%, transparent)",
        ].join(", "),

    "--os3d-primary-button-stack-active": dark
      ? [
          "inset 0 2px 6px color-mix(in srgb, #000 28%, transparent)",
          "inset 0 1px 0 color-mix(in srgb, #000 16%, transparent)",
          "0 1px 2px -1px color-mix(in srgb, #000 16%, transparent)",
        ].join(", ")
      : [
          "inset 0 2px 6px color-mix(in srgb, var(--primary) 20%, #000)",
          "inset 0 1px 0 color-mix(in srgb, #fff 8%, transparent)",
        ].join(", "),

    /** 危险色填充：与 chip-lift 同构，阴影色相跟 `--semantic-danger`。 */
    "--os3d-danger-button-stack": dark
      ? [
          "inset 0 1px 0 color-mix(in srgb, var(--primary-fg) 10%, transparent)",
          "0 4px 14px -6px color-mix(in srgb, #000 32%, transparent)",
          "0 1px 3px -1px color-mix(in srgb, #000 18%, transparent)",
        ].join(", ")
      : [
          "inset 0 1px 0 color-mix(in srgb, var(--primary-fg) 12%, transparent)",
          "inset 0 16px 20px -20px color-mix(in srgb, var(--primary-fg) 6%, transparent)",
          "0 10px 20px -14px color-mix(in srgb, var(--semantic-danger) 18%, transparent)",
          "0 2px 6px -4px color-mix(in srgb, var(--semantic-danger) 10%, transparent)",
        ].join(", "),

    "--os3d-danger-button-stack-hover": dark
      ? [
          "inset 0 1px 0 color-mix(in srgb, var(--primary-fg) 14%, transparent)",
          "0 6px 18px -6px color-mix(in srgb, #000 36%, transparent)",
          "0 2px 5px -2px color-mix(in srgb, #000 22%, transparent)",
        ].join(", ")
      : [
          "inset 0 1px 0 color-mix(in srgb, var(--primary-fg) 14%, transparent)",
          "inset 0 18px 22px -22px color-mix(in srgb, var(--primary-fg) 8%, transparent)",
          "0 12px 22px -14px color-mix(in srgb, var(--semantic-danger) 20%, transparent)",
          "0 3px 7px -5px color-mix(in srgb, var(--semantic-danger) 12%, transparent)",
        ].join(", "),

    "--os3d-danger-button-stack-active": dark
      ? [
          "inset 0 2px 6px color-mix(in srgb, #000 30%, transparent)",
          "inset 0 1px 0 color-mix(in srgb, #000 18%, transparent)",
          "0 1px 2px -1px color-mix(in srgb, #000 18%, transparent)",
        ].join(", ")
      : [
          "inset 0 2px 6px color-mix(in srgb, var(--semantic-danger) 22%, #000)",
          "inset 0 1px 0 color-mix(in srgb, #fff 7%, transparent)",
        ].join(", "),
  };
}
