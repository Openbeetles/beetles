export type ThemeMode = 'light' | 'dark'
/** 默认品牌 `logo`：与侧栏矢量甲壳虫标 + favicon 主色体系对齐 */
export type ThemeBrand = 'blue' | 'teal' | 'logo' | 'firmware'

/**
 * 布局与动效 Token（单源，与 mode/brand 无关）。
 * 禁止在 theme、组件内写死圆角/间距/时长，必须引用此处或 :root 变量。
 */
export const LAYOUT_TOKENS = {
  /** 控件圆角（按钮、输入框、Toggle 等） */
  radiusControl: 4,
  /** 卡片/抽屉/弹层圆角 */
  radiusCard: 6,
  /** 小控件圆角（Chip、IconButton、Tooltip） */
  radiusChip: 4,
  /** 强调动效曲线 */
  easeEmphasized: 'cubic-bezier(0.22, 1, 0.36, 1)',
  /** 平滑缓动曲线 */
  easeOutSmooth: 'cubic-bezier(0.25, 0.1, 0.25, 1)',
  /** 卡片内图片 hover 动画时长（ms） */
  durationImageHoverMs: 380,
  /** 按钮默认最小高度 */
  buttonMinHeight: 38,
  buttonMinHeightLarge: 46,
  buttonMinHeightSmall: 30,
  /** 大按钮水平内边距 */
  buttonPaddingXLarge: 24,
  /** CardContent 内边距 */
  cardContentPadding: 18,
  /** ToggleButtonGroup 间距 */
  toggleGroupGap: 4,
  /** ToggleButton 上下内边距 */
  toggleButtonPaddingY: 8,
  /** Tooltip 内边距 */
  tooltipPadding: '8px 12px',
  /** 焦点环宽度 */
  focusRingWidth: 2,
  focusRingOffset: 2,
  /** Hero 主标题字号 */
  heroTitleFontSize: '2.25rem',
  /** Hero 副标题字号 */
  heroSubtitleFontSize: '1.0625rem',
  /** Hero 区域垂直间距（theme spacing 倍数） */
  heroSpacingY: 6,
  /** Hero 装饰线宽/高（px） */
  heroAccentWidth: 56,
  heroAccentHeight: 4,
  /** 列表/卡片入场错落延迟（ms），用于 stagger 动效 */
  staggerStepMs: 60,
  /** 搜索框等 pill 形态圆角（px），足够大即呈全圆角 */
  radiusSearchPill: 9999,
  /** 强调线宽度（左侧/顶部主色条，区块 accent） */
  accentLineWidth: 2,
  /** 卡片顶部强调线宽度（较克制） */
  cardAccentLineWidth: 2,
  /** 图标尺寸：小（列表内、输入框内） */
  iconSizeSm: 20,
  /** 图标尺寸：中（导航、区块内） */
  iconSizeMd: 24,
  /** 图标尺寸：大（Logo、区块图标容器） */
  iconSizeLg: 32,
  /** 图标容器尺寸：小（导航 Logo） */
  iconContainerSm: 30,
  /** 图标容器尺寸：中（设置抽屉标题、Section 数字） */
  iconContainerMd: 36,
  /** 图标容器尺寸：大（分类卡片图标） */
  iconContainerLg: 40,
  /** 图标容器尺寸：大（Agent 头像） */
  iconContainerXl: 46,
  /** 装饰圆点直径（px，Section 标题下小点） */
  dotDecorationPx: 4,
  /** 装饰线高度（px，Section 标题下渐变线） */
  accentLineHeight: 2,
  /** 装饰线宽度（px，Section 标题下短线） */
  accentLineShortWidth: 24,
  /** 装饰线宽度（px，Section 标题下长线） */
  accentLineLongWidth: 32,
  /** 装饰圆点直径（px） */
  dotSizePx: 6,
  /** 轮播/指示点直径（px） */
  indicatorDotSizePx: 8,
  /** 轮播指示点激活态宽度（px，pill 形态） */
  indicatorDotActiveWidthPx: 20,
  /** 轮播 slide 切换时长（ms） */
  carouselSlideDurationMs: 520,
  /** 轮播与右侧资讯卡叠压宽度（px） */
  carouselOverlapPx: 24,
  /**
   * 全屏状态遮罩（重启中、离线缓存等）的 backdrop blur（px）。
   * 与导航栏 `glassBlur` 解耦：遮罩需更强分离度。
   */
  overlayBackdropBlurPx: 12,
  /** 顶栏下状态卡片最大宽度（px），与 `calc(100vw - gutter)` 配合 */
  statusOverlayCardMaxPx: 520,
  /** 状态卡片相对视口水平安全边距（px） */
  statusOverlayCardInsetPx: 24,
  /** 页面主标题下渐变装饰条长度（px） */
  pageHeaderAccentBarWidthPx: 48,
  /** 窄幅确认弹窗内容最大宽度（px），与 MUI maxWidth xs 搭配 */
  dialogNarrowMaxWidthPx: 360,
  /** hover 上浮位移（px），用于卡片等；控制台面板固定为 0 */
  hoverLiftY: 0,
  /** hover 右移位移（px），用于“更多”链接、箭头等 */
  hoverShiftX: 2,
  /** 字间距：标题紧 */
  letterSpacingTight: '-0.025em',
  /** 字间距：标签/上标 */
  letterSpacingLabel: '0.04em',

  // ---------- 字号与行高（单源，保证层次与呼吸感） ----------
  /** 字号：Display（Hero 主标题） */
  fontSizeDisplay: '2.25rem',
  /** 字号：H1 */
  fontSizeH1: '1.75rem',
  /** 字号：H2 / 区块主标题 */
  fontSizeH2: '1.5rem',
  /** 字号：H3 */
  fontSizeH3: '1.25rem',
  /** 字号：H4 / 卡片主标题 */
  fontSizeH4: '1.125rem',
  /** 字号：正文大（副标题、引导） */
  fontSizeBodyLg: '1.0625rem',
  /** 字号：正文 */
  fontSizeBody: '0.9375rem',
  /** 字号：正文小 */
  fontSizeBodySm: '0.875rem',
  /** 字号：说明 / 辅助 */
  fontSizeCaption: '0.8125rem',
  /** 字号：上标 / 标签小字 */
  fontSizeOverline: '0.75rem',
  /** 字号：徽章 / 极小标签 */
  fontSizeLabel: '0.6875rem',
  /** 等宽字体栈：数值、地址、标识符 */
  fontMono:
    '"JetBrains Mono", "Fira Code", "Cascadia Code", ui-monospace, monospace',
  /** 等宽数据值字号 */
  fontSizeDataValue: '0.8125rem',
  /** 行高：紧（大标题） */
  lineHeightTight: 1.2,
  /** 行高：略紧（小标题、卡片标题） */
  lineHeightSnug: 1.35,
  /** 行高：正文 */
  lineHeightNormal: 1.5,
  /** 行高：略松（长正文、副标题） */
  lineHeightRelaxed: 1.6,
  /** 行高：更松（长说明、法律/风险提示类段落） */
  lineHeightLoose: 1.75,
} as const

/** 主题 Token：所有 UI 颜色必须由此映射，禁止在组件内硬编码色值。 */
export interface ThemeTokens {
  background: string
  foreground: string
  card: string
  /** 略高于 background 的表面层 */
  surface: string
  muted: string
  border: string
  primary: string
  primarySoft: string
  /** 主色上的文字色 */
  primaryFg: string
  accent: string
  imageOverlay: string
  overlay: string
  /** 导航栏毛玻璃背景（白/清透明） */
  appBarGlass: string
  backdropOverlay: string
  glassBlur: string
  /** 统一动效时长，组件 transition 必须引用 */
  transitionDuration: string
  /** 强调动效时长（卡片 hover、导航切换等，略长以增强高级感） */
  transitionDurationEmphasized: string
  foregroundSoft: string
  borderSubtle: string
  /** 极轻阴影（仅用于悬浮等克制的层次） */
  shadowSubtle: string
  /** 卡片 hover 时极轻阴影（略强于 shadowSubtle，仍克制） */
  shadowCardHover: string
  /** NEW/新品等标识用红色系 */
  badgeNew: string
}

/** 语义色：与主题/品牌无关，用于评分高低、积分、警告等；偏淡以保持清爽 */
export const SEMANTIC_COLORS = {
  /** 成功（保存成功、连接正常等状态） */
  success: '#22c55e',
  /** 危险/错误（失败、异常、中断） */
  danger: '#f87171',
  /** 警告（需关注但非致命） */
  warning: '#f59e0b',
  /** 积分默认金黄色 */
  points: '#ca8a04',
} as const

/** 品牌主色（设置抽屉色块选择器用）；略淡以保持清爽；firmware 与固件内置配置页 common.css 的 --primary 一致 */
export const BRAND_COLORS: Record<ThemeBrand, string> = {
  blue: '#3b82f6',
  teal: '#14b8a6',
  /** Logo 主色域：壳面深紫（与 logo 视觉一致） */
  logo: '#6d28d9',
  firmware: '#c43030',
}

/** 设置里展示顺序：默认品牌 logo 放首位 */
export const THEME_BRAND_KEYS: ThemeBrand[] = ['logo', 'blue', 'teal', 'firmware']

const tokenMap: Record<ThemeMode, Record<ThemeBrand, ThemeTokens>> = {
  light: {
    blue: {
      background: '#ffffff',
      foreground: '#2d3142',
      card: '#ffffff',
      surface: '#f8f9fc',
      muted: '#64748b',
      border: '#e8ecf2',
      primary: '#3b82f6',
      primarySoft: 'rgba(59, 130, 246, 0.06)',
      primaryFg: '#ffffff',
      accent: '#60a5fa',
      imageOverlay: 'rgba(0, 0, 0, 0.50)',
      overlay: 'rgba(255, 255, 255, 0.92)',
      appBarGlass: 'rgba(255, 255, 255, 0.72)',
      backdropOverlay: 'rgba(0, 0, 0, 0.28)',
      glassBlur: '24px',
      transitionDuration: '200ms',
      transitionDurationEmphasized: '220ms',
      foregroundSoft: '#64748b',
      borderSubtle: '#f4f6f9',
      shadowSubtle: '0 1px 0 rgba(15,23,42,0.035)',
      shadowCardHover: '0 4px 20px rgba(15,23,42,0.045)',
      badgeNew: '#ef4444',
    },
    teal: {
      background: '#ffffff',
      foreground: '#2d3142',
      card: '#ffffff',
      surface: '#f8f9fc',
      muted: '#64748b',
      border: '#e8ecf2',
      primary: '#14b8a6',
      primarySoft: 'rgba(20, 184, 166, 0.06)',
      primaryFg: '#ffffff',
      accent: '#2dd4bf',
      imageOverlay: 'rgba(0, 0, 0, 0.50)',
      overlay: 'rgba(255, 255, 255, 0.92)',
      appBarGlass: 'rgba(255, 255, 255, 0.72)',
      backdropOverlay: 'rgba(0, 0, 0, 0.28)',
      glassBlur: '24px',
      transitionDuration: '200ms',
      transitionDurationEmphasized: '220ms',
      foregroundSoft: '#64748b',
      borderSubtle: '#f4f6f9',
      shadowSubtle: '0 1px 0 rgba(15,23,42,0.035)',
      shadowCardHover: '0 4px 20px rgba(15,23,42,0.045)',
      badgeNew: '#ef4444',
    },
    logo: {
      background: '#ffffff',
      foreground: '#2d3142',
      card: '#ffffff',
      surface: '#f7f5ff',
      muted: '#64748b',
      border: '#ebe7f3',
      primary: '#6d28d9',
      primarySoft: 'rgba(109, 40, 217, 0.08)',
      primaryFg: '#ffffff',
      accent: '#0891b2',
      imageOverlay: 'rgba(0, 0, 0, 0.50)',
      overlay: 'rgba(255, 255, 255, 0.92)',
      appBarGlass: 'rgba(255, 255, 255, 0.72)',
      backdropOverlay: 'rgba(0, 0, 0, 0.28)',
      glassBlur: '24px',
      transitionDuration: '200ms',
      transitionDurationEmphasized: '220ms',
      foregroundSoft: '#64748b',
      borderSubtle: '#f5f3fb',
      shadowSubtle: '0 1px 0 rgba(15,23,42,0.035)',
      shadowCardHover: '0 4px 20px rgba(15,23,42,0.045)',
      badgeNew: '#ef4444',
    },
    firmware: {
      background: '#ffffff',
      foreground: '#2d3142',
      card: '#ffffff',
      surface: '#f8f9fc',
      muted: '#64748b',
      border: '#e8ecf2',
      primary: '#c43030',
      primarySoft: 'rgba(196, 48, 48, 0.06)',
      primaryFg: '#ffffff',
      accent: '#dc6b6b',
      imageOverlay: 'rgba(0, 0, 0, 0.50)',
      overlay: 'rgba(255, 255, 255, 0.92)',
      appBarGlass: 'rgba(255, 255, 255, 0.72)',
      backdropOverlay: 'rgba(0, 0, 0, 0.28)',
      glassBlur: '24px',
      transitionDuration: '200ms',
      transitionDurationEmphasized: '220ms',
      foregroundSoft: '#64748b',
      borderSubtle: '#f4f6f9',
      shadowSubtle: '0 1px 0 rgba(15,23,42,0.035)',
      shadowCardHover: '0 4px 20px rgba(15,23,42,0.045)',
      badgeNew: '#ef4444',
    },
  },
  dark: {
    blue: {
      background: '#111318',
      foreground: '#e2e8f0',
      card: '#1a1d24',
      surface: '#22262e',
      muted: '#94a3b8',
      border: '#2c3340',
      primary: '#60a5fa',
      primarySoft: 'rgba(59, 130, 246, 0.10)',
      primaryFg: '#ffffff',
      accent: '#93c5fd',
      imageOverlay: 'rgba(0, 0, 0, 0.55)',
      overlay: 'rgba(17, 19, 24, 0.92)',
      appBarGlass: 'rgba(26, 29, 36, 0.75)',
      backdropOverlay: 'rgba(0, 0, 0, 0.50)',
      glassBlur: '24px',
      transitionDuration: '200ms',
      transitionDurationEmphasized: '220ms',
      foregroundSoft: '#94a3b8',
      borderSubtle: '#1a1e26',
      shadowSubtle: '0 1px 0 rgba(0,0,0,0.35)',
      shadowCardHover: '0 4px 18px rgba(0,0,0,0.22)',
      badgeNew: '#f87171',
    },
    teal: {
      background: '#111318',
      foreground: '#e2e8f0',
      card: '#1a1d24',
      surface: '#22262e',
      muted: '#94a3b8',
      border: '#2c3340',
      primary: '#2dd4bf',
      primarySoft: 'rgba(20, 184, 166, 0.10)',
      primaryFg: '#ffffff',
      accent: '#5eead4',
      imageOverlay: 'rgba(0, 0, 0, 0.55)',
      overlay: 'rgba(17, 19, 24, 0.92)',
      appBarGlass: 'rgba(26, 29, 36, 0.75)',
      backdropOverlay: 'rgba(0, 0, 0, 0.50)',
      glassBlur: '24px',
      transitionDuration: '200ms',
      transitionDurationEmphasized: '220ms',
      foregroundSoft: '#94a3b8',
      borderSubtle: '#1a1e26',
      shadowSubtle: '0 1px 0 rgba(0,0,0,0.35)',
      shadowCardHover: '0 4px 18px rgba(0,0,0,0.22)',
      badgeNew: '#f87171',
    },
    logo: {
      background: '#0f0b18',
      foreground: '#e8e4f5',
      card: '#161024',
      surface: '#1c1530',
      muted: '#94a3b8',
      border: '#302448',
      primary: '#a78bfa',
      primarySoft: 'rgba(167, 139, 250, 0.12)',
      primaryFg: '#ffffff',
      accent: '#22d3ee',
      imageOverlay: 'rgba(0, 0, 0, 0.55)',
      overlay: 'rgba(15, 11, 24, 0.92)',
      appBarGlass: 'rgba(22, 16, 36, 0.75)',
      backdropOverlay: 'rgba(0, 0, 0, 0.50)',
      glassBlur: '24px',
      transitionDuration: '200ms',
      transitionDurationEmphasized: '220ms',
      foregroundSoft: '#94a3b8',
      borderSubtle: '#221830',
      shadowSubtle: '0 1px 0 rgba(0,0,0,0.35)',
      shadowCardHover: '0 4px 18px rgba(0,0,0,0.22)',
      badgeNew: '#f87171',
    },
    firmware: {
      background: '#111318',
      foreground: '#e2e8f0',
      card: '#1a1d24',
      surface: '#22262e',
      muted: '#94a3b8',
      border: '#2c3340',
      primary: '#ef7a7a',
      primarySoft: 'rgba(196, 48, 48, 0.08)',
      primaryFg: '#ffffff',
      accent: '#fca5a5',
      imageOverlay: 'rgba(0, 0, 0, 0.55)',
      overlay: 'rgba(17, 19, 24, 0.92)',
      appBarGlass: 'rgba(26, 29, 36, 0.75)',
      backdropOverlay: 'rgba(0, 0, 0, 0.50)',
      glassBlur: '24px',
      transitionDuration: '200ms',
      transitionDurationEmphasized: '220ms',
      foregroundSoft: '#94a3b8',
      borderSubtle: '#1a1e26',
      shadowSubtle: '0 1px 0 rgba(0,0,0,0.35)',
      shadowCardHover: '0 4px 18px rgba(0,0,0,0.22)',
      badgeNew: '#f87171',
    },
  },
}

export function getThemeTokens(mode: ThemeMode, brand: ThemeBrand): ThemeTokens {
  return tokenMap[mode][brand]
}
