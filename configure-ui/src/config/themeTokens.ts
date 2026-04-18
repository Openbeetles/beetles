export type ThemeMode = 'light' | 'dark'
/** 默认品牌 `logo`：与 Beetle OS 矢量徽标 + favicon 主色体系对齐 */
export type ThemeBrand = 'blue' | 'teal' | 'logo' | 'firmware'

/**
 * 布局与动效 Token（单源，与 mode/brand 无关）。
 * 禁止在 theme、组件内写死圆角/间距/时长，必须引用此处或 :root 变量。
 */
export const LAYOUT_TOKENS = {
  /** 控件圆角（按钮、输入框、Toggle 等） */
  radiusControl: 10,
  /** 卡片/抽屉/弹层圆角 */
  radiusCard: 16,
  /** 小控件圆角（Chip、IconButton、Tooltip） */
  radiusChip: 10,
  /** 强调动效曲线 */
  easeEmphasized: 'cubic-bezier(0.22, 1, 0.36, 1)',
  /** 平滑缓动曲线 */
  easeOutSmooth: 'cubic-bezier(0.25, 0.1, 0.25, 1)',
  /** 卡片内图片 hover 动画时长（ms） */
  durationImageHoverMs: 380,
  /** 按钮默认最小高度 */
  buttonMinHeight: 42,
  buttonMinHeightLarge: 50,
  buttonMinHeightSmall: 32,
  /** 大按钮水平内边距 */
  buttonPaddingXLarge: 28,
  /** CardContent 内边距 */
  cardContentPadding: 24,
  /** ToggleButtonGroup 间距 */
  toggleGroupGap: 6,
  /** ToggleButton 上下内边距 */
  toggleButtonPaddingY: 10,
  /** Tooltip 内边距 */
  tooltipPadding: '10px 14px',
  /** 焦点环宽度 */
  focusRingWidth: 2,
  focusRingOffset: 2,
  /** Hero 主标题字号 */
  heroTitleFontSize: '2.5rem',
  /** Hero 副标题字号 */
  heroSubtitleFontSize: '1.125rem',
  /** Hero 区域垂直间距（theme spacing 倍数） */
  heroSpacingY: 8,
  /** Hero 装饰线宽/高（px） */
  heroAccentWidth: 56,
  heroAccentHeight: 4,
  /** 列表/卡片入场错落延迟（ms），用于 stagger 动效 */
  staggerStepMs: 60,
  /** 搜索框等 pill 形态圆角（px），足够大即呈全圆角 */
  radiusSearchPill: 9999,
  /** 强调线宽度（左侧/顶部主色条，区块 accent） */
  accentLineWidth: 3,
  /** 卡片顶部强调线宽度（较克制） */
  cardAccentLineWidth: 3,
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
  /**
   * SettingsSection 内「空/加载说明/只读提示」插画外框与内图（px）。
   * Panel empty-state / status hero; matches full-screen disconnect card weight.
   */
  panelStateIconWellPx: 52,
  panelStateIconInnerPx: 40,
  /** 紧凑条（如「请先连接设备」） */
  panelStateIconWellCompactPx: 44,
  panelStateIconInnerCompactPx: 32,
  /** 装饰圆点直径（px，Section 标题下小点） */
  dotDecorationPx: 4,
  /** 装饰线高度（px，Section 标题下渐变线、Tabs 指示条） */
  accentLineHeight: 3,
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
  /**
   * 顶栏/任务栏等壳层磨砂的 blur（px），略轻于全屏遮罩，保留更多底图纹理。
   */
  shellChromeBackdropBlurPx: 12,
  /** 顶栏下状态卡片最大宽度（px），与 `calc(100vw - gutter)` 配合 */
  statusOverlayCardMaxPx: 440,
  /** 状态卡片相对视口水平安全边距（px） */
  statusOverlayCardInsetPx: 24,
  /** 页面主标题下渐变装饰条长度（px） */
  pageHeaderAccentBarWidthPx: 48,
  /** 窄幅确认弹窗内容最大宽度（px），与 MUI maxWidth xs 搭配 */
  dialogNarrowMaxWidthPx: 320,
  /** hover 上浮位移（px），用于卡片等；控制台面板固定为 0 */
  hoverLiftY: 0,
  /** hover 右移位移（px），用于“更多”链接、箭头等 */
  hoverShiftX: 2,
  /** 字间距：标题紧 */
  letterSpacingTight: '-0.025em',
  /** 字间距：标签/上标 */
  letterSpacingLabel: '0.04em',

  // ---------- 字号与行高（单源：偏「桌面 OS」体量，层次拉开、留白充足） ----------
  /** 字号：Display（Hero 主标题） */
  fontSizeDisplay: '2.5rem',
  /** 字号：H1 */
  fontSizeH1: '2rem',
  /** 字号：H2 / 区块主标题 */
  fontSizeH2: '1.625rem',
  /** 字号：H3 */
  fontSizeH3: '1.375rem',
  /** 字号：H4 / 卡片主标题 */
  fontSizeH4: '1.25rem',
  /** 字号：正文大（副标题、引导） */
  fontSizeBodyLg: '1.125rem',
  /** 字号：正文（基准 16px，避免「网页感」偏小正文） */
  fontSizeBody: '1rem',
  /** 字号：正文小 */
  fontSizeBodySm: '0.9375rem',
  /** 字号：说明 / 辅助 */
  fontSizeCaption: '0.875rem',
  /** 字号：上标 / 标签小字 */
  fontSizeOverline: '0.8125rem',
  /** 字号：徽章 / 极小标签 */
  fontSizeLabel: '0.75rem',
  /** 等宽字体栈：数值、地址、标识符 */
  fontMono:
    '"JetBrains Mono", "Fira Code", "Cascadia Code", ui-monospace, monospace',
  /** 等宽数据值字号 */
  fontSizeDataValue: '0.875rem',
  /** 行高：紧（大标题） */
  lineHeightTight: 1.18,
  /** 行高：略紧（小标题、卡片标题） */
  lineHeightSnug: 1.38,
  /** 行高：正文 */
  lineHeightNormal: 1.55,
  /** 行高：略松（长正文、副标题） */
  lineHeightRelaxed: 1.68,
  /** 行高：更松（长说明、法律/风险提示类段落） */
  lineHeightLoose: 1.8,

  // ---------- 垂直节奏（MUI `theme.spacing` 倍数，默认 8px/单位）----------
  /** 页根：InlineAlert 与 Section、Section 与 Section */
  spacingPageStack: 2,
  /** 大卡片并排或同列多块之间的 gap（与 `spacingPageStack` 同阶时可复用） */
  spacingSectionStack: 2,
  /** 表单项纵向：`FormFieldStack`、折叠块内多字段 */
  spacingFormFields: 2.5,
  /** 紧密：行内图标+文字、chip 旁说明 */
  spacingInlineTight: 1,
  /** 标题行与下方首段内容（略紧于 section gap） */
  spacingTitleToContent: 1.5,
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
  /** 历史字段：扁平系统设置风下恒为 `none`，层次靠描边与底色区分 */
  shadowSubtle: string
  /** 历史字段：扁平风下恒为 `none` */
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

/**
 * 浅色模式页面画布：浅青灰（非纯白、非暖灰），清爽耐看；`card` 仍为白以托内容。
 */
const LIGHT_PAGE_BACKGROUND = '#eef4f9'

/** 壳层 / 次级表面：比画布略亮、偏冷，仍属青灰白 */
const LIGHT_SURFACE_COOL = '#fbfdff'

/** 浅色文字层级：secondary 用于正文辅助，tertiary 用于 id / metadata，必须明显分层。 */
const LIGHT_TEXT_SECONDARY = '#66778d'
const LIGHT_TEXT_TERTIARY = '#95a3b5'

/** 深色文字层级：secondary 仍可读，tertiary 退到 metadata 级，避免整页一片同亮度。 */
const DARK_TEXT_SECONDARY = '#b2bfd0'
const DARK_TEXT_TERTIARY = '#8794a8'

const tokenMap: Record<ThemeMode, Record<ThemeBrand, ThemeTokens>> = {
  light: {
    blue: {
      background: LIGHT_PAGE_BACKGROUND,
      foreground: '#2d3142',
      card: '#ffffff',
      surface: LIGHT_SURFACE_COOL,
      muted: LIGHT_TEXT_TERTIARY,
      border: '#dbe4ec',
      primary: '#3b82f6',
      primarySoft: 'rgba(59, 130, 246, 0.06)',
      primaryFg: '#ffffff',
      accent: '#60a5fa',
      imageOverlay: 'rgba(0, 0, 0, 0.50)',
      overlay: 'rgba(255, 255, 255, 0.94)',
      appBarGlass: 'rgba(250, 252, 255, 0.82)',
      backdropOverlay: 'rgba(0, 0, 0, 0.28)',
      glassBlur: '24px',
      transitionDuration: '200ms',
      transitionDurationEmphasized: '220ms',
      foregroundSoft: LIGHT_TEXT_SECONDARY,
      borderSubtle: '#edf2f6',
      shadowSubtle:
        '0 22px 44px -32px color-mix(in srgb, var(--foreground) 18%, transparent)',
      shadowCardHover:
        '0 26px 52px -30px color-mix(in srgb, var(--foreground) 20%, transparent)',
      badgeNew: '#ef4444',
    },
    teal: {
      background: LIGHT_PAGE_BACKGROUND,
      foreground: '#2d3142',
      card: '#ffffff',
      surface: LIGHT_SURFACE_COOL,
      muted: LIGHT_TEXT_TERTIARY,
      border: '#dbe4ec',
      primary: '#14b8a6',
      primarySoft: 'rgba(20, 184, 166, 0.06)',
      primaryFg: '#ffffff',
      accent: '#2dd4bf',
      imageOverlay: 'rgba(0, 0, 0, 0.50)',
      overlay: 'rgba(255, 255, 255, 0.94)',
      appBarGlass: 'rgba(248, 252, 252, 0.82)',
      backdropOverlay: 'rgba(0, 0, 0, 0.28)',
      glassBlur: '24px',
      transitionDuration: '200ms',
      transitionDurationEmphasized: '220ms',
      foregroundSoft: LIGHT_TEXT_SECONDARY,
      borderSubtle: '#edf2f6',
      shadowSubtle:
        '0 22px 44px -32px color-mix(in srgb, var(--foreground) 18%, transparent)',
      shadowCardHover:
        '0 26px 52px -30px color-mix(in srgb, var(--foreground) 20%, transparent)',
      badgeNew: '#ef4444',
    },
    logo: {
      background: LIGHT_PAGE_BACKGROUND,
      foreground: '#2d3142',
      card: '#ffffff',
      surface: '#f7f5ff',
      muted: LIGHT_TEXT_TERTIARY,
      border: '#ebe7f3',
      primary: '#6d28d9',
      primarySoft: 'rgba(109, 40, 217, 0.08)',
      primaryFg: '#ffffff',
      accent: '#0891b2',
      imageOverlay: 'rgba(0, 0, 0, 0.50)',
      overlay: 'rgba(255, 255, 255, 0.94)',
      appBarGlass: 'rgba(251, 249, 255, 0.82)',
      backdropOverlay: 'rgba(0, 0, 0, 0.28)',
      glassBlur: '24px',
      transitionDuration: '200ms',
      transitionDurationEmphasized: '220ms',
      foregroundSoft: LIGHT_TEXT_SECONDARY,
      borderSubtle: '#f5f3fb',
      shadowSubtle:
        '0 22px 44px -32px color-mix(in srgb, var(--foreground) 18%, transparent)',
      shadowCardHover:
        '0 26px 52px -30px color-mix(in srgb, var(--foreground) 20%, transparent)',
      badgeNew: '#ef4444',
    },
    firmware: {
      background: LIGHT_PAGE_BACKGROUND,
      foreground: '#2d3142',
      card: '#ffffff',
      surface: LIGHT_SURFACE_COOL,
      muted: LIGHT_TEXT_TERTIARY,
      border: '#dbe4ec',
      primary: '#c43030',
      primarySoft: 'rgba(196, 48, 48, 0.06)',
      primaryFg: '#ffffff',
      accent: '#dc6b6b',
      imageOverlay: 'rgba(0, 0, 0, 0.50)',
      overlay: 'rgba(255, 255, 255, 0.94)',
      appBarGlass: 'rgba(251, 250, 250, 0.82)',
      backdropOverlay: 'rgba(0, 0, 0, 0.28)',
      glassBlur: '24px',
      transitionDuration: '200ms',
      transitionDurationEmphasized: '220ms',
      foregroundSoft: LIGHT_TEXT_SECONDARY,
      borderSubtle: '#edf2f6',
      shadowSubtle:
        '0 22px 44px -32px color-mix(in srgb, var(--foreground) 18%, transparent)',
      shadowCardHover:
        '0 26px 52px -30px color-mix(in srgb, var(--foreground) 20%, transparent)',
      badgeNew: '#ef4444',
    },
  },
  dark: {
    blue: {
      background: '#111318',
      foreground: '#e2e8f0',
      card: '#1a1d24',
      surface: '#22262e',
      muted: DARK_TEXT_TERTIARY,
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
      foregroundSoft: DARK_TEXT_SECONDARY,
      borderSubtle: '#1a1e26',
      shadowSubtle:
        '0 18px 40px -26px color-mix(in srgb, #000 38%, transparent)',
      shadowCardHover:
        '0 24px 52px -22px color-mix(in srgb, #000 42%, transparent)',
      badgeNew: '#f87171',
    },
    teal: {
      background: '#111318',
      foreground: '#e2e8f0',
      card: '#1a1d24',
      surface: '#22262e',
      muted: DARK_TEXT_TERTIARY,
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
      foregroundSoft: DARK_TEXT_SECONDARY,
      borderSubtle: '#1a1e26',
      shadowSubtle:
        '0 18px 40px -26px color-mix(in srgb, #000 38%, transparent)',
      shadowCardHover:
        '0 24px 52px -22px color-mix(in srgb, #000 42%, transparent)',
      badgeNew: '#f87171',
    },
    logo: {
      background: '#0f0b18',
      foreground: '#e8e4f5',
      card: '#161024',
      surface: '#1c1530',
      muted: DARK_TEXT_TERTIARY,
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
      foregroundSoft: DARK_TEXT_SECONDARY,
      borderSubtle: '#221830',
      shadowSubtle:
        '0 18px 40px -26px color-mix(in srgb, #000 40%, transparent)',
      shadowCardHover:
        '0 24px 52px -22px color-mix(in srgb, #000 44%, transparent)',
      badgeNew: '#f87171',
    },
    firmware: {
      background: '#111318',
      foreground: '#e2e8f0',
      card: '#1a1d24',
      surface: '#22262e',
      muted: DARK_TEXT_TERTIARY,
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
      foregroundSoft: DARK_TEXT_SECONDARY,
      borderSubtle: '#1a1e26',
      shadowSubtle:
        '0 18px 40px -26px color-mix(in srgb, #000 38%, transparent)',
      shadowCardHover:
        '0 24px 52px -22px color-mix(in srgb, #000 42%, transparent)',
      badgeNew: '#f87171',
    },
  },
}

export function getThemeTokens(mode: ThemeMode, brand: ThemeBrand): ThemeTokens {
  return tokenMap[mode][brand]
}
