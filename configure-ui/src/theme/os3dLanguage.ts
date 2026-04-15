import type { ThemeMode } from "../config/themeTokens";

/**
 * Beetle OS「拟物 3D」单源：凹 / 凸 分层 — 大板与卡内槽用 **凹陷承托**，
 * 数字块、胶囊、区块图标用 **凸起贴件**（上沿高光 + 底侧柔影），避免全盘 inset。
 * 体积光仅用 **foreground / 中性黑** 混色，避免与品牌主色叠出「脏边」。
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
  | "inline";

/** CSS 变量名 → 供 `Os3dIcon` 与文档引用 */
export const OS3D_ICON_FILTER_VAR: Record<Os3dIconVariant, string> = {
  default: "var(--os3d-icon-filter-default)",
  dock: "var(--os3d-icon-filter-dock)",
  tile: "var(--os3d-icon-filter-tile)",
  hero: "var(--os3d-icon-filter-hero)",
  inline: "var(--os3d-icon-filter-inline)",
};

export function os3dRootCssVars(mode: ThemeMode): Record<string, string> {
  const dark = mode === "dark";

  /**
   * 图标：三层柔光叠化 — 远距弥散 + 中距塑形 + 贴底接触线；半径偏大、alpha 偏低。
   */
  const iconDefault = dark
    ? [
        "drop-shadow(0 10px 22px rgba(0,0,0,0.26))",
        "drop-shadow(0 4px 10px rgba(0,0,0,0.16))",
        "drop-shadow(0 1px 2px rgba(0,0,0,0.12))",
      ].join(" ")
    : [
        "drop-shadow(0 12px 28px color-mix(in srgb, var(--foreground) 9%, transparent))",
        "drop-shadow(0 5px 12px color-mix(in srgb, var(--foreground) 6%, transparent))",
        "drop-shadow(0 1px 3px color-mix(in srgb, var(--foreground) 5%, transparent))",
      ].join(" ");

  const iconDock = dark
    ? [
        "drop-shadow(0 7px 16px rgba(0,0,0,0.2))",
        "drop-shadow(0 2px 5px rgba(0,0,0,0.12))",
      ].join(" ")
    : [
        "drop-shadow(0 8px 18px color-mix(in srgb, var(--foreground) 7%, transparent))",
        "drop-shadow(0 2px 5px color-mix(in srgb, var(--foreground) 5%, transparent))",
      ].join(" ");

  const iconTile = dark
    ? [
        "drop-shadow(0 12px 28px rgba(0,0,0,0.3))",
        "drop-shadow(0 5px 12px rgba(0,0,0,0.16))",
        "drop-shadow(0 2px 4px rgba(0,0,0,0.1))",
      ].join(" ")
    : [
        "drop-shadow(0 14px 32px color-mix(in srgb, var(--foreground) 11%, transparent))",
        "drop-shadow(0 6px 14px color-mix(in srgb, var(--foreground) 7%, transparent))",
        "drop-shadow(0 2px 4px color-mix(in srgb, var(--foreground) 5%, transparent))",
      ].join(" ");

  const iconHero = dark
    ? [
        "drop-shadow(0 18px 40px rgba(0,0,0,0.32))",
        "drop-shadow(0 8px 18px rgba(0,0,0,0.18))",
        "drop-shadow(0 2px 5px rgba(0,0,0,0.12))",
      ].join(" ")
    : [
        "drop-shadow(0 20px 44px color-mix(in srgb, var(--foreground) 12%, transparent))",
        "drop-shadow(0 8px 20px color-mix(in srgb, var(--foreground) 8%, transparent))",
        "drop-shadow(0 3px 6px color-mix(in srgb, var(--foreground) 5%, transparent))",
      ].join(" ");

  const iconInline = dark
    ? "drop-shadow(0 4px 10px rgba(0,0,0,0.18)) drop-shadow(0 1px 2px rgba(0,0,0,0.1))"
    : [
        "drop-shadow(0 5px 12px color-mix(in srgb, var(--foreground) 6%, transparent))",
        "drop-shadow(0 1px 2px color-mix(in srgb, var(--foreground) 4%, transparent))",
      ].join(" ");

  /** 深色：环境影用一点 foreground 混色，避免死黑；浅色：全用混色柔边 */
  const plateDark = [
    "inset 0 1px 0 color-mix(in srgb, var(--foreground) 7%, transparent)",
    "0 16px 44px -12px color-mix(in srgb, var(--foreground) 14%, transparent)",
    "0 6px 16px -4px color-mix(in srgb, #000 22%, transparent)",
    "0 1px 0 color-mix(in srgb, var(--foreground) 4%, transparent)",
  ].join(", ");

  const plateLight = [
    "inset 0 1px 0 color-mix(in srgb, var(--foreground) 6%, transparent)",
    "inset 0 -1px 0 color-mix(in srgb, var(--foreground) 3%, transparent)",
    "0 14px 40px -14px color-mix(in srgb, var(--foreground) 9%, transparent)",
    "0 4px 12px -2px color-mix(in srgb, var(--foreground) 5%, transparent)",
  ].join(", ");

  return {
    "--os3d-icon-filter-default": iconDefault,
    "--os3d-icon-filter-dock": iconDock,
    "--os3d-icon-filter-tile": iconTile,
    "--os3d-icon-filter-hero": iconHero,
    "--os3d-icon-filter-inline": iconInline,

    /** 顶栏：极薄顶缘光，像磨砂边缘而非硬线 */
    "--os3d-chrome-titlebar-stack": [
      "inset 0 1px 0 color-mix(in srgb, var(--foreground) 7%, transparent)",
      "inset 0 -1px 0 color-mix(in srgb, var(--foreground) 2.5%, transparent)",
    ].join(", "),

    /** 任务栏：柔光上托，扩散大、强度低 */
    "--os3d-chrome-taskbar-stack": [
      "inset 0 1px 0 color-mix(in srgb, var(--foreground) 6%, transparent)",
      "0 -8px 28px -10px color-mix(in srgb, var(--foreground) 5%, transparent)",
      "0 -2px 8px -2px color-mix(in srgb, var(--foreground) 3%, transparent)",
    ].join(", "),

    "--shell-main-inset-top":
      "inset 0 1px 0 color-mix(in srgb, var(--foreground) 3.5%, transparent)",

    /** 插画井：更浅的碟形，底缘几乎只作分隔暗示 */
    "--os3d-icon-well-dish": [
      "inset 0 1px 0 color-mix(in srgb, var(--foreground) 5.5%, transparent)",
      "inset 0 -1px 0 color-mix(in srgb, var(--foreground) 2.5%, transparent)",
    ].join(", "),

    "--os3d-content-plate-stack": dark ? plateDark : plateLight,

    /** 仪表盘卡顶栏：极淡的「蜡面」上缘 */
    "--os3d-dashboard-card-header-lip":
      "inset 0 1px 0 color-mix(in srgb, var(--foreground) 3.5%, transparent)",

    /**
     * 仪表盘卡正文相对标题栏：浅「屏坑」承托内容（标题栏略抬、正文略沉）。
     */
    "--os3d-dashboard-body-recess": dark
      ? "inset 0 3px 9px color-mix(in srgb, #000 12%, transparent)"
      : "inset 0 2px 7px color-mix(in srgb, var(--foreground) 5%, transparent)",

    /**
     * 配置子导航列表区：落在左栏大板内的浅槽（左栏仍用 content-plate 抬起）。
     */
    "--os3d-subnav-track-recess": dark
      ? [
          "inset 0 2px 6px color-mix(in srgb, #000 11%, transparent)",
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 3%, transparent)",
        ].join(", ")
      : "inset 0 2px 5px color-mix(in srgb, var(--foreground) 4%, transparent)",

    "--os3d-banner-ribbon-stack": [
      "inset 0 1px 0 color-mix(in srgb, var(--foreground) 5.5%, transparent)",
      "0 6px 22px -8px color-mix(in srgb, var(--foreground) 5%, transparent)",
    ].join(", "),

    /** 语义条：以「浮起丝带」为主（外影略强），顶缘仅薄蜡光，不全凹 */
    "--os3d-alert-strip-stack": dark
      ? [
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 5%, transparent)",
          "0 5px 18px -5px color-mix(in srgb, var(--foreground) 12%, transparent)",
          "0 2px 6px -2px color-mix(in srgb, #000 14%, transparent)",
        ].join(", ")
      : [
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 6%, transparent)",
          "0 5px 16px -6px color-mix(in srgb, var(--foreground) 8%, transparent)",
          "0 2px 5px -1px color-mix(in srgb, var(--foreground) 4%, transparent)",
        ].join(", "),

    /**
     * 压在凹底/卡面上的「贴件」凸起：仪表盘数字块、通道 StatPill 等。
     * Raised chip: top wax + ambient below — reads above inset panels.
     */
    "--os3d-chip-lift-stack": dark
      ? [
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 9%, transparent)",
          "0 4px 12px -4px color-mix(in srgb, #000 26%, transparent)",
          "0 1px 3px -1px color-mix(in srgb, var(--foreground) 5%, transparent)",
        ].join(", ")
      : [
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 11%, transparent)",
          "0 4px 11px -4px color-mix(in srgb, var(--foreground) 7%, transparent)",
          "0 1px 2px color-mix(in srgb, var(--foreground) 4%, transparent)",
        ].join(", "),

    /**
     * 设置区标题旁图标槽、空态插画井：比 chip 更轻的承托凸起（小台座）。
     */
    "--os3d-pedestal-lift-stack": dark
      ? [
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 7%, transparent)",
          "0 3px 10px -4px color-mix(in srgb, #000 20%, transparent)",
        ].join(", ")
      : [
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 9%, transparent)",
          "0 3px 9px -3px color-mix(in srgb, var(--foreground) 6%, transparent)",
        ].join(", "),

    /** 顶栏面包屑胶囊：轻浮于磨砂条之上 */
    "--os3d-breadcrumb-lift-stack": [
      "inset 0 1px 0 color-mix(in srgb, var(--foreground) 4%, transparent)",
      "inset 0 -1px 0 color-mix(in srgb, var(--foreground) 1.5%, transparent)",
      "0 2px 9px -3px color-mix(in srgb, var(--foreground) 5%, transparent)",
    ].join(", "),

    /** 卡内嵌套：凹影更浅、边更柔 */
    "--os3d-inset-panel-stack": dark
      ? [
          "inset 0 3px 10px color-mix(in srgb, #000 18%, transparent)",
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 4%, transparent)",
        ].join(", ")
      : [
          "inset 0 3px 8px color-mix(in srgb, var(--foreground) 6%, transparent)",
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 3.5%, transparent)",
        ].join(", "),

    /**
     * 真·凹格（极少用）：仅适合需「按下/沉孔」语义的槽位；一般控件优先 `chip-lift`。
     */
    "--os3d-micro-well-stack": dark
      ? [
          "inset 0 2px 5px color-mix(in srgb, #000 20%, transparent)",
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 3%, transparent)",
        ].join(", ")
      : [
          "inset 0 2px 4px color-mix(in srgb, var(--foreground) 7%, transparent)",
          "inset 0 1px 0 color-mix(in srgb, var(--foreground) 3%, transparent)",
        ].join(", "),
  };
}
