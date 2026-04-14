import { createElement } from 'react'
import { createTheme } from '@mui/material'
import {
  BRAND_COLORS,
  getThemeTokens,
  LAYOUT_TOKENS,
  SEMANTIC_COLORS,
  THEME_BRAND_KEYS,
  type ThemeBrand,
  type ThemeMode,
} from '../config/themeTokens'
import { CONTENT_MAX_WIDTH } from '../config/layout'
import {
  OsCheckboxCheckedIcon,
  OsCheckboxIcon,
  OsCheckboxIndeterminateIcon,
  OsRadioCheckedIcon,
  OsRadioIcon,
} from './osFormControlIcons'

const R = LAYOUT_TOKENS.radiusControl
const R_CARD = LAYOUT_TOKENS.radiusCard
const R_CHIP = LAYOUT_TOKENS.radiusChip

/** 预留：卡片内图片不再做 hover 缩放，避免无意义动效 */
export const cardImageHoverSx = {} as const

export function createAppTheme(mode: ThemeMode, brand: ThemeBrand) {
  const tokens = getThemeTokens(mode, brand)

  return createTheme({
    cssVariables: true,
    breakpoints: {
      values: {
        xs: 0,
        sm: 600,
        md: 900,
        lg: CONTENT_MAX_WIDTH,
        xl: 1536,
      },
    },
    palette: {
      mode,
      primary: {
        main: tokens.primary,
        contrastText: tokens.primaryFg,
      },
      secondary: {
        main: tokens.accent,
      },
      background: {
        default: tokens.background,
        paper: tokens.card,
      },
      text: {
        primary: tokens.foreground,
        secondary: tokens.muted,
      },
      divider: tokens.border,
    },
    shape: {
      borderRadius: R,
    },
    typography: {
      fontFamily: 'var(--font-sans)',
      /** 与 `--font-size-body` 对齐（16px），rem 组件尺寸一致 */
      fontSize: 16,
      h1: {
        fontSize: LAYOUT_TOKENS.fontSizeH1,
        fontWeight: 700,
        letterSpacing: '-0.04em',
        lineHeight: LAYOUT_TOKENS.lineHeightTight,
      },
      h2: {
        fontSize: LAYOUT_TOKENS.fontSizeH2,
        fontWeight: 700,
        letterSpacing: '-0.03em',
        lineHeight: LAYOUT_TOKENS.lineHeightTight,
      },
      h3: {
        fontSize: LAYOUT_TOKENS.fontSizeH3,
        fontWeight: 700,
        letterSpacing: '-0.025em',
        lineHeight: LAYOUT_TOKENS.lineHeightSnug,
      },
      h4: {
        fontSize: LAYOUT_TOKENS.fontSizeH4,
        fontWeight: 700,
        letterSpacing: '-0.02em',
        lineHeight: LAYOUT_TOKENS.lineHeightSnug,
      },
      h5: {
        fontSize: LAYOUT_TOKENS.fontSizeBodyLg,
        fontWeight: 600,
        letterSpacing: '-0.015em',
        lineHeight: LAYOUT_TOKENS.lineHeightSnug,
      },
      h6: {
        fontSize: LAYOUT_TOKENS.fontSizeH4,
        fontWeight: 600,
        letterSpacing: '-0.01em',
        lineHeight: LAYOUT_TOKENS.lineHeightSnug,
      },
      subtitle1: {
        fontSize: LAYOUT_TOKENS.fontSizeBody,
        fontWeight: 500,
        lineHeight: LAYOUT_TOKENS.lineHeightNormal,
      },
      subtitle2: {
        fontSize: LAYOUT_TOKENS.fontSizeBodySm,
        fontWeight: 600,
        lineHeight: LAYOUT_TOKENS.lineHeightSnug,
      },
      body1: {
        fontSize: LAYOUT_TOKENS.fontSizeBody,
        fontWeight: 400,
        lineHeight: LAYOUT_TOKENS.lineHeightRelaxed,
      },
      body2: {
        fontSize: LAYOUT_TOKENS.fontSizeBodySm,
        fontWeight: 400,
        lineHeight: LAYOUT_TOKENS.lineHeightNormal,
      },
      caption: {
        fontSize: LAYOUT_TOKENS.fontSizeCaption,
        fontWeight: 400,
        lineHeight: LAYOUT_TOKENS.lineHeightNormal,
      },
      button: { fontWeight: 600 },
    },
    components: {
      MuiCssBaseline: {
        styleOverrides: {
          ':root': {
            '--font-brand':
              "'Orbitron', 'IBM Plex Sans', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif",
            scrollBehavior: 'smooth',
            '--background': tokens.background,
            '--foreground': tokens.foreground,
            '--card': tokens.card,
            '--surface': tokens.surface,
            '--muted': tokens.muted,
            '--border': tokens.border,
            /**
             * 表单层次（派生自 border + card，随品牌/深浅切换）：
             * 浅色下 Section 多为白 card，嵌套分组与输入框需更明显「井」与描边。
             */
            '--form-group-well': `color-mix(in srgb, ${tokens.border} ${mode === 'light' ? 14 : 10}%, ${tokens.card})`,
            '--input-idle-well': `color-mix(in srgb, ${tokens.border} ${mode === 'light' ? 12 : 6}%, ${tokens.card})`,
            '--form-outline-rest':
              mode === 'light'
                ? 'color-mix(in srgb, var(--border) 26%, transparent)'
                : 'color-mix(in srgb, var(--border) 20%, transparent)',
            '--outlined-border-rest':
              mode === 'light'
                ? 'color-mix(in srgb, var(--border) 36%, transparent)'
                : 'color-mix(in srgb, var(--border) 28%, transparent)',
            '--outlined-border-hover':
              mode === 'light'
                ? 'color-mix(in srgb, var(--border) 48%, transparent)'
                : 'color-mix(in srgb, var(--border) 42%, transparent)',
            '--primary': tokens.primary,
            '--primary-soft': tokens.primarySoft,
            '--primary-fg': tokens.primaryFg,
            '--accent': tokens.accent,
            '--image-overlay': tokens.imageOverlay,
            '--overlay': tokens.overlay,
            '--app-bar-glass': tokens.appBarGlass,
            '--backdrop-overlay': tokens.backdropOverlay,
            '--glass-blur': tokens.glassBlur,
            '--transition-duration': tokens.transitionDuration,
            '--transition-duration-emphasized': tokens.transitionDurationEmphasized,
            '--radius-control': `${LAYOUT_TOKENS.radiusControl}px`,
            '--radius-card': `${LAYOUT_TOKENS.radiusCard}px`,
            '--radius-chip': `${LAYOUT_TOKENS.radiusChip}px`,
            '--foreground-soft': tokens.foregroundSoft,
            /**
             * 文字色阶（与 `foreground` / `foreground-soft` / `muted` 对齐，命名便于组件统一引用）。
             * Text hierarchy aliases for consistent Typography `color`.
             */
            '--text-primary': tokens.foreground,
            '--text-secondary': tokens.foregroundSoft,
            '--text-tertiary': tokens.muted,
            '--border-subtle': tokens.borderSubtle,
            '--ease-emphasized': LAYOUT_TOKENS.easeEmphasized,
            '--ease-out-smooth': LAYOUT_TOKENS.easeOutSmooth,
            '--focus-ring-width': `${LAYOUT_TOKENS.focusRingWidth}px`,
            '--focus-ring-offset': `${LAYOUT_TOKENS.focusRingOffset}px`,
            '--hero-title-font-size': LAYOUT_TOKENS.heroTitleFontSize,
            '--hero-subtitle-font-size': LAYOUT_TOKENS.heroSubtitleFontSize,
            '--hero-spacing-y': LAYOUT_TOKENS.heroSpacingY,
            '--hero-accent-width': `${LAYOUT_TOKENS.heroAccentWidth}px`,
            '--hero-accent-height': `${LAYOUT_TOKENS.heroAccentHeight}px`,
            '--stagger-step-ms': `${LAYOUT_TOKENS.staggerStepMs}ms`,
            '--radius-search-pill': `${LAYOUT_TOKENS.radiusSearchPill}px`,
            /** 全圆角（pill 按钮、状态 chip）；与 `radiusSearchPill` 同语义，便于组件直写 */
            '--radius-full': '9999px',
            /** 列表行、横幅底边等弱分割线与 SettingsRow 一致 */
            '--divider-row':
              '1px solid color-mix(in srgb, var(--border) 18%, transparent)',
            '--accent-line-width': `${LAYOUT_TOKENS.accentLineWidth}px`,
            '--card-accent-line-width': `${LAYOUT_TOKENS.cardAccentLineWidth}px`,
            '--icon-size-sm': `${LAYOUT_TOKENS.iconSizeSm}px`,
            '--icon-size-md': `${LAYOUT_TOKENS.iconSizeMd}px`,
            '--icon-size-lg': `${LAYOUT_TOKENS.iconSizeLg}px`,
            '--icon-container-sm': `${LAYOUT_TOKENS.iconContainerSm}px`,
            '--icon-container-md': `${LAYOUT_TOKENS.iconContainerMd}px`,
            '--icon-container-lg': `${LAYOUT_TOKENS.iconContainerLg}px`,
            '--icon-container-xl': `${LAYOUT_TOKENS.iconContainerXl}px`,
            '--dot-size': `${LAYOUT_TOKENS.dotSizePx}px`,
            '--dot-decoration': `${LAYOUT_TOKENS.dotDecorationPx}px`,
            '--accent-line-height': `${LAYOUT_TOKENS.accentLineHeight}px`,
            '--accent-line-short-width': `${LAYOUT_TOKENS.accentLineShortWidth}px`,
            '--accent-line-long-width': `${LAYOUT_TOKENS.accentLineLongWidth}px`,
            '--indicator-dot-size': `${LAYOUT_TOKENS.indicatorDotSizePx}px`,
            '--indicator-dot-active-width': `${LAYOUT_TOKENS.indicatorDotActiveWidthPx}px`,
            '--carousel-slide-duration': `${LAYOUT_TOKENS.carouselSlideDurationMs}ms`,
            '--carousel-overlap': `${LAYOUT_TOKENS.carouselOverlapPx}px`,
            '--overlay-backdrop-blur': `${LAYOUT_TOKENS.overlayBackdropBlurPx}px`,
            '--shell-chrome-blur': `${LAYOUT_TOKENS.shellChromeBackdropBlurPx}px`,
            '--status-overlay-card-max': `${LAYOUT_TOKENS.statusOverlayCardMaxPx}px`,
            '--status-overlay-card-inset': `${LAYOUT_TOKENS.statusOverlayCardInsetPx}px`,
            '--page-header-accent-width': `${LAYOUT_TOKENS.pageHeaderAccentBarWidthPx}px`,
            '--dialog-narrow-max-width': `${LAYOUT_TOKENS.dialogNarrowMaxWidthPx}px`,
            '--hover-lift-y': `${LAYOUT_TOKENS.hoverLiftY}px`,
            '--hover-shift-x': `${LAYOUT_TOKENS.hoverShiftX}px`,
            '--letter-spacing-tight': LAYOUT_TOKENS.letterSpacingTight,
            '--letter-spacing-label': LAYOUT_TOKENS.letterSpacingLabel,
            '--shadow-subtle': tokens.shadowSubtle,
            '--shadow-card': tokens.shadowSubtle,
            '--shadow-card-hover': tokens.shadowCardHover,
            /** 顶栏：不外投阴影，层次靠边框 + 磨砂哑光底（无背景渐变） */
            '--shadow-shell-titlebar': 'none',
            /** 侧栏：扁平，与主区靠右边框分隔（见 Layout） */
            '--shadow-shell-rail': 'none',
            /** 主内容区顶缘：不外投/内凹阴影，与顶栏仅靠边框分隔 */
            '--shell-main-inset-top': 'none',
            /** 浮动面板（设置抽屉等）：无投影，靠左边框与主区分隔 */
            '--shadow-shell-floating': 'none',
            /** 任务栏：不外投阴影，与顶栏一致 */
            '--shadow-shell-taskbar': 'none',
            /** 开始菜单弹出层：扁平，不外投阴影（与壳层一致） */
            '--shadow-shell-start-flyout': 'none',
            '--font-mono': LAYOUT_TOKENS.fontMono,
            '--font-size-data-value': LAYOUT_TOKENS.fontSizeDataValue,
            '--badge-new': tokens.badgeNew,
            '--semantic-success': SEMANTIC_COLORS.success,
            '--semantic-danger': SEMANTIC_COLORS.danger,
            '--semantic-warning': SEMANTIC_COLORS.warning,
            '--warning': SEMANTIC_COLORS.warning,
            '--points': SEMANTIC_COLORS.points,
            '--font-size-display': LAYOUT_TOKENS.fontSizeDisplay,
            '--font-size-h1': LAYOUT_TOKENS.fontSizeH1,
            '--font-size-h2': LAYOUT_TOKENS.fontSizeH2,
            '--font-size-h3': LAYOUT_TOKENS.fontSizeH3,
            '--font-size-h4': LAYOUT_TOKENS.fontSizeH4,
            '--font-size-body-lg': LAYOUT_TOKENS.fontSizeBodyLg,
            '--font-size-body': LAYOUT_TOKENS.fontSizeBody,
            '--font-size-body-sm': LAYOUT_TOKENS.fontSizeBodySm,
            '--font-size-caption': LAYOUT_TOKENS.fontSizeCaption,
            '--font-size-overline': LAYOUT_TOKENS.fontSizeOverline,
            '--font-size-label': LAYOUT_TOKENS.fontSizeLabel,
            '--line-height-tight': String(LAYOUT_TOKENS.lineHeightTight),
            '--line-height-snug': String(LAYOUT_TOKENS.lineHeightSnug),
            '--line-height-normal': String(LAYOUT_TOKENS.lineHeightNormal),
            '--line-height-relaxed': String(LAYOUT_TOKENS.lineHeightRelaxed),
            '--line-height-loose': String(LAYOUT_TOKENS.lineHeightLoose),
            ...Object.fromEntries(
              THEME_BRAND_KEYS.map((b) => [`--brand-${b}`, BRAND_COLORS[b]]),
            ),
          },
          body: {
            backgroundColor: 'var(--background)',
            color: 'var(--foreground)',
            WebkitFontSmoothing: 'antialiased',
            MozOsxFontSmoothing: 'grayscale',
            textRendering: 'geometricPrecision',
            cursor: 'default',
          },
          '*:focus-visible': {
            outline: `2px solid color-mix(in srgb, var(--primary) 62%, transparent)`,
            outlineOffset: LAYOUT_TOKENS.focusRingOffset,
          },
        },
      },
      MuiTextField: {
        defaultProps: {
          size: 'medium',
        },
      },
      MuiFormControl: {
        defaultProps: {
          size: 'medium',
        },
      },
      MuiDrawer: {
        styleOverrides: {
          paper: {
            transition:
              'transform 280ms var(--ease-emphasized), box-shadow var(--transition-duration) ease, border-color var(--transition-duration) ease',
          },
        },
      },
      MuiButton: {
        defaultProps: { disableElevation: true, disableRipple: true },
        styleOverrides: {
          root: {
            textTransform: 'none',
            fontWeight: 600,
            borderRadius: R,
            boxShadow: 'none',
            minHeight: LAYOUT_TOKENS.buttonMinHeight,
            transition: 'color var(--transition-duration) ease, background-color var(--transition-duration) ease, border-color var(--transition-duration) ease, transform var(--transition-duration) var(--ease-emphasized)',
          },
          contained: {
            backgroundColor: 'var(--primary)',
            color: 'var(--primary-fg)',
            borderTop: '1px solid color-mix(in srgb, var(--primary-fg) 12%, transparent)',
            '&:hover': {
              backgroundColor: 'color-mix(in srgb, var(--primary) 92%, white)',
              boxShadow: 'none',
            },
            '&:active': {
              backgroundColor: 'color-mix(in srgb, var(--primary) 85%, white)',
              borderTopColor: 'transparent',
              boxShadow: 'none',
            },
          },
          /** 否则 contained 会盖住 MUI 的 color=error，确认类危险操作无法显示红色 */
          containedError: {
            backgroundColor: 'var(--semantic-danger)',
            color: 'var(--primary-fg)',
            borderTop: '1px solid color-mix(in srgb, var(--primary-fg) 12%, transparent)',
            '&:hover': {
              backgroundColor: 'color-mix(in srgb, var(--semantic-danger) 88%, black)',
              boxShadow: 'none',
            },
            '&:active': {
              backgroundColor: 'color-mix(in srgb, var(--semantic-danger) 80%, black)',
              borderTopColor: 'transparent',
              boxShadow: 'none',
            },
          },
          outlined: {
            borderColor: 'color-mix(in srgb, var(--primary) 20%, var(--border))',
            '&:hover': {
              borderColor: 'color-mix(in srgb, var(--primary) 38%, var(--border))',
              backgroundColor: 'color-mix(in srgb, var(--primary) 5%, transparent)',
            },
          },
          text: {
            color: 'var(--muted)',
            '&:hover': {
              backgroundColor: 'color-mix(in srgb, var(--primary) 6%, transparent)',
              color: 'var(--primary)',
            },
          },
          sizeLarge: {
            minHeight: LAYOUT_TOKENS.buttonMinHeightLarge,
            fontSize: '1rem',
            paddingLeft: LAYOUT_TOKENS.buttonPaddingXLarge,
            paddingRight: LAYOUT_TOKENS.buttonPaddingXLarge,
          },
          sizeSmall: {
            minHeight: LAYOUT_TOKENS.buttonMinHeightSmall,
            fontSize: 'var(--font-size-caption)',
          },
        },
      },
      MuiAppBar: {
        styleOverrides: {
          root: {
            boxShadow: 'none',
            borderBottom: '1px solid var(--border-subtle)',
            backdropFilter: 'blur(var(--glass-blur))',
            WebkitBackdropFilter: 'blur(var(--glass-blur))',
            backgroundColor: 'var(--app-bar-glass)',
            transition: 'background-color var(--transition-duration) ease, border-color var(--transition-duration) ease',
          },
        },
      },
      MuiPaper: {
        styleOverrides: {
          root: {
            boxShadow: 'none',
            border: 'none',
            backgroundImage: 'none',
          },
        },
      },
      MuiCard: {
        styleOverrides: {
          root: {
            borderRadius: R_CARD,
            border: '1px solid var(--form-outline-rest)',
            boxShadow: 'none',
            backgroundColor: 'var(--card)',
          },
        },
      },
      MuiTabs: {
        styleOverrides: {
          root: {
            backgroundColor: 'var(--card)',
            borderBottom: '1px solid var(--form-outline-rest)',
          },
          indicator: {
            height: 'var(--accent-line-height)',
          },
        },
      },
      MuiTab: {
        styleOverrides: {
          root: {
            textTransform: 'none',
            fontWeight: 600,
            fontSize: 'var(--font-size-body)',
            minHeight: 48,
            color: 'var(--muted)',
            '&.Mui-selected': {
              color: 'var(--primary)',
            },
          },
        },
      },
      MuiCardContent: {
        styleOverrides: {
          root: {
            padding: LAYOUT_TOKENS.cardContentPadding,
            '&:last-child': { paddingBottom: LAYOUT_TOKENS.cardContentPadding },
          },
        },
      },
      MuiInputLabel: {
        styleOverrides: {
          root: {
            '&.Mui-disabled': {
              color: 'var(--text-tertiary)',
              opacity: 1,
            },
          },
        },
      },
      MuiOutlinedInput: {
        styleOverrides: {
          root: {
            borderRadius: R,
            backgroundColor: 'var(--input-idle-well)',
            transition: 'background-color var(--transition-duration) ease',
            '& .MuiOutlinedInput-notchedOutline': {
              borderColor: 'var(--outlined-border-rest)',
              transition: 'border-color var(--transition-duration-emphasized) var(--ease-emphasized), border-width var(--transition-duration) ease',
            },
            '& .MuiInputBase-input::placeholder': {
              opacity: 0.7,
              color: 'var(--muted)',
            },
            '&:hover .MuiOutlinedInput-notchedOutline': {
              borderColor: 'var(--outlined-border-hover)',
            },
            '&.Mui-focused': {
              backgroundColor: 'var(--card)',
              '& .MuiOutlinedInput-notchedOutline': {
                borderColor: 'color-mix(in srgb, var(--primary) 38%, var(--border))',
                borderWidth: 1,
              },
            },
            /**
             * 禁用态：贴近卡片底、弱描边（默认 disabled 偏闷、偏深）。
             * Disabled: lighter well + subtler outline than MUI default.
             */
            '&.Mui-disabled': {
              opacity: 1,
              backgroundColor:
                'color-mix(in srgb, var(--card) 94%, var(--foreground))',
              '& .MuiOutlinedInput-notchedOutline': {
                borderColor:
                  'color-mix(in srgb, var(--border) 12%, transparent)',
              },
              '& .MuiInputBase-input': {
                color: 'var(--text-tertiary)',
                WebkitTextFillColor: 'var(--text-tertiary)',
                opacity: 1,
              },
            },
          },
        },
      },
      MuiChip: {
        styleOverrides: {
          root: {
            borderRadius: R_CHIP,
            fontWeight: 600,
            fontSize: 'var(--font-size-caption)',
            transition: 'background-color var(--transition-duration) ease, border-color var(--transition-duration) ease, color var(--transition-duration) ease',
          },
          outlined: {
            borderColor: 'color-mix(in srgb, var(--border) 28%, transparent)',
            '&:hover': {
              backgroundColor: 'color-mix(in srgb, var(--primary) 4%, transparent)',
              borderColor: 'color-mix(in srgb, var(--border) 40%, transparent)',
              color: 'var(--primary)',
            },
          },
        },
      },
      MuiIconButton: {
        defaultProps: { disableRipple: true },
        styleOverrides: {
          root: {
            borderRadius: R_CHIP,
            transition: 'color var(--transition-duration) ease, background-color var(--transition-duration) ease, transform var(--transition-duration) ease',
            '&:hover': {
              backgroundColor: 'color-mix(in srgb, var(--primary) 6%, transparent)',
              color: 'var(--primary)',
            },
            '&:active': { transform: 'scale(0.96)' },
            '&:focus-visible': {
              outline: 'var(--focus-ring-width) solid var(--primary)',
              outlineOffset: 'var(--focus-ring-offset)',
            },
            '@media (prefers-reduced-motion: reduce)': {
              '&:active': { transform: 'none' },
            },
          },
        },
      },
      MuiDivider: {
        styleOverrides: {
          root: {
            borderColor: 'color-mix(in srgb, var(--border) 20%, transparent)',
          },
        },
      },
      /**
       * 开关：井底轨道 + 内凹阴影；滑块顶高光模拟 3D，选中轨与拇指同主色系。
       * Switch — inset track well, glossy thumb; checked state aligns with primary.
       */
      MuiSwitch: {
        defaultProps: { disableRipple: true },
        styleOverrides: {
          /**
           * MUI Root 默认 `overflow:hidden` 会裁掉滑块描边/外阴影（尤其未选态靠左时）。
           * 已用收紧的 `input` 尺寸，可改为 visible 避免圆角被「切直边」。
           */
          root: {
            overflow: 'visible',
          },
          /**
           * MUI 默认将 `input` 设为 `width:300%; left:-100%` 以扩大触控区，会导致
           * DevTools/布局测量出现远大于轨道的「幽灵宽度」并像溢出父级。收紧到 switchBase 内。
           * Default oversized invisible input is constrained to the thumb hit area.
           */
          switchBase: {
            color: 'var(--card)',
            '& .MuiSwitch-input': {
              left: 0,
              width: '100%',
              height: '100%',
            },
            '& .MuiSwitch-thumb': {
              backgroundColor: 'var(--card)',
              backgroundImage:
                'linear-gradient(180deg, color-mix(in srgb, var(--foreground) 14%, transparent) 0%, transparent 58%)',
              border:
                '1px solid color-mix(in srgb, var(--foreground) 18%, transparent)',
              boxShadow:
                '0 1px 3px color-mix(in srgb, var(--foreground) 32%, transparent), inset 0 1px 0 color-mix(in srgb, var(--foreground) 20%, transparent)',
            },
            '&.Mui-checked': {
              color: 'var(--primary)',
              '& .MuiSwitch-thumb': {
                backgroundImage:
                  'linear-gradient(180deg, color-mix(in srgb, var(--primary-fg) 38%, transparent) 0%, transparent 55%)',
                border:
                  '1px solid color-mix(in srgb, var(--primary) 50%, transparent)',
                boxShadow:
                  '0 1px 3px color-mix(in srgb, var(--primary) 30%, transparent), inset 0 1px 0 color-mix(in srgb, var(--primary-fg) 28%, transparent)',
              },
              '& + .MuiSwitch-track': {
                backgroundColor: 'color-mix(in srgb, var(--primary) 40%, transparent)',
                border:
                  '1px solid color-mix(in srgb, var(--primary) 48%, transparent)',
                opacity: 1,
                boxShadow:
                  'inset 0 1px 2px color-mix(in srgb, var(--foreground) 10%, transparent)',
              },
            },
            '&.Mui-focusVisible': {
              outline: 'var(--focus-ring-width) solid var(--primary)',
              outlineOffset: 2,
              borderRadius: 999,
            },
            '@media (prefers-reduced-motion: reduce)': {
              '& .MuiSwitch-thumb': {
                transition: 'none',
              },
            },
          },
          track: {
            opacity: 1,
            borderRadius: 999,
            border: '1px solid var(--outlined-border-rest)',
            backgroundColor: 'var(--input-idle-well)',
            boxShadow:
              'inset 0 1px 2px color-mix(in srgb, var(--foreground) 9%, transparent)',
          },
        },
      },
      /**
       * MUI 默认 `marginLeft:-11`（与 labelPlacement start 时 `marginRight:-11`）用于与 TextField 标签对齐，
       * 会把 Switch/Checkbox 拉出卡片内容区，在 `overflow:auto` 的 Section 里裁切滑块描边。
       * Remove negative margins; use flex gap for control/label spacing.
       */
      MuiFormControlLabel: {
        styleOverrides: {
          root: {
            marginLeft: 0,
            marginRight: 16,
            columnGap: 8,
            '&.MuiFormControlLabel-labelPlacementStart': {
              marginRight: 0,
              marginLeft: 16,
              columnGap: 8,
            },
            '&.MuiFormControlLabel-labelPlacementTop, &.MuiFormControlLabel-labelPlacementBottom':
              {
                marginLeft: 16,
              },
          },
        },
      },
      /** 矢量井字 + 对勾，与 `--input-idle-well` 一致 */
      MuiCheckbox: {
        defaultProps: {
          disableRipple: true,
          color: 'primary',
          icon: createElement(OsCheckboxIcon),
          checkedIcon: createElement(OsCheckboxCheckedIcon),
          indeterminateIcon: createElement(OsCheckboxIndeterminateIcon),
        },
        styleOverrides: {
          root: {
            padding: 8,
            color: 'var(--muted)',
            borderRadius: R,
            transition:
              'background-color var(--transition-duration) ease, box-shadow var(--transition-duration) ease',
            '&:hover': {
              backgroundColor:
                'color-mix(in srgb, var(--primary) 5%, transparent)',
            },
            '&.Mui-focusVisible': {
              outline: 'var(--focus-ring-width) solid var(--primary)',
              outlineOffset: 'var(--focus-ring-offset)',
            },
            '&.Mui-checked, &.MuiCheckbox-indeterminate': {
              color: 'var(--primary)',
            },
            '&.Mui-disabled': {
              opacity: 0.42,
            },
          },
        },
      },
      MuiRadio: {
        defaultProps: {
          disableRipple: true,
          color: 'primary',
          icon: createElement(OsRadioIcon),
          checkedIcon: createElement(OsRadioCheckedIcon),
        },
        styleOverrides: {
          root: {
            padding: 8,
            borderRadius: '50%',
            color: 'var(--muted)',
            '&:hover': {
              backgroundColor:
                'color-mix(in srgb, var(--primary) 5%, transparent)',
            },
            '&.Mui-focusVisible': {
              outline: 'var(--focus-ring-width) solid var(--primary)',
              outlineOffset: 'var(--focus-ring-offset)',
            },
            '&.Mui-checked': { color: 'var(--primary)' },
            '&.Mui-disabled': { opacity: 0.42 },
          },
        },
      },
      /** Select / 右键菜单等：与卡片同一套描边，无 Material 浮影 */
      MuiMenu: {
        styleOverrides: {
          paper: {
            marginTop: 1,
            borderRadius: R_CARD,
            border: '1px solid var(--form-outline-rest)',
            boxShadow: 'none',
            backgroundColor: 'var(--card)',
            backgroundImage: 'none',
            paddingTop: 8,
            paddingBottom: 8,
          },
        },
      },
      MuiPopover: {
        styleOverrides: {
          paper: {
            borderRadius: R_CARD,
            border: '1px solid var(--form-outline-rest)',
            boxShadow: 'none',
            backgroundColor: 'var(--card)',
            backgroundImage: 'none',
          },
        },
      },
      MuiDialog: {
        styleOverrides: {
          paper: {
            borderRadius: R_CARD,
            border: '1px solid var(--form-outline-rest)',
            boxShadow: 'none',
            backgroundColor: 'var(--card)',
            backgroundImage: 'none',
          },
        },
      },
      /**
       * 配置侧栏、设置式子导航等与系统设置列表一致：无 ripple、选中用主色浅底。
       * Settings-style nav rows (ConfigSubNavLayout); keep layout-only sx in the caller.
       */
      MuiListItemButton: {
        defaultProps: { disableRipple: true },
        styleOverrides: {
          root: {
            borderRadius: R,
            color: 'var(--foreground)',
            transition:
              'background-color var(--transition-duration) ease, color var(--transition-duration) ease',
            '&:hover': {
              backgroundColor: 'color-mix(in srgb, var(--foreground) 4%, transparent)',
            },
            '&:focus-visible': {
              outline: 'var(--focus-ring-width) solid var(--primary)',
              outlineOffset: 2,
            },
            '&.Mui-selected': {
              backgroundColor: 'color-mix(in srgb, var(--primary) 10%, transparent)',
              color: 'var(--primary)',
              '&:hover': {
                backgroundColor: 'color-mix(in srgb, var(--primary) 14%, transparent)',
              },
            },
          },
        },
      },
      /** Toast（Snackbar）语义色仍由 ToastProvider 注入，结构与字体由主题统一 */
      MuiSnackbarContent: {
        styleOverrides: {
          root: {
            borderRadius: R,
            fontWeight: 600,
            fontSize: 'var(--font-size-body-sm)',
            boxShadow: 'none',
            border: '1px solid',
            borderColor: 'transparent',
          },
        },
      },
      MuiSlider: {
        styleOverrides: {
          root: {
            color: 'var(--primary)',
            height: 4,
          },
          rail: {
            opacity: 1,
            borderRadius: 1,
            backgroundColor: 'color-mix(in srgb, var(--foreground) 9%, transparent)',
          },
          track: {
            border: 'none',
            borderRadius: 1,
          },
          thumb: {
            width: 18,
            height: 18,
            borderRadius: R,
            backgroundColor: 'var(--card)',
            border: '2px solid var(--primary)',
            '&:hover, &.Mui-focusVisible': {
              boxShadow: '0 0 0 6px color-mix(in srgb, var(--primary) 22%, transparent)',
            },
          },
        },
      },
      MuiTooltip: {
        defaultProps: { arrow: true },
        styleOverrides: {
          tooltip: {
            backgroundColor: 'var(--foreground)',
            color: 'var(--background)',
            fontSize: 'var(--font-size-caption)',
            fontWeight: 500,
            borderRadius: R_CHIP,
            padding: LAYOUT_TOKENS.tooltipPadding,
          },
          arrow: { color: 'var(--foreground)' },
        },
      },
      MuiToggleButtonGroup: {
        styleOverrides: {
          root: { gap: LAYOUT_TOKENS.toggleGroupGap },
          grouped: {
            border: '1px solid color-mix(in srgb, var(--border) 22%, transparent)',
            borderRadius: 'var(--radius-control) !important',
            textTransform: 'none',
            fontWeight: 600,
            fontSize: 'var(--font-size-body-sm)',
            paddingTop: LAYOUT_TOKENS.toggleButtonPaddingY,
            paddingBottom: LAYOUT_TOKENS.toggleButtonPaddingY,
            transition:
              'background-color var(--transition-duration) ease, color var(--transition-duration) ease, border-color var(--transition-duration) ease',
            backgroundColor: 'transparent',
            color: 'var(--muted)',
            '&.Mui-selected': {
              backgroundColor: 'var(--primary-soft)',
              color: 'var(--primary)',
              borderColor: 'color-mix(in srgb, var(--primary) 35%, var(--border))',
              '&:hover': {
                backgroundColor: 'color-mix(in srgb, var(--primary) 6%, var(--primary-soft))',
              },
            },
            '&:hover': {
              backgroundColor: 'color-mix(in srgb, var(--foreground) 3%, transparent)',
              color: 'var(--foreground)',
            },
          },
        },
      },
      MuiLink: {
        styleOverrides: {
          root: {
            color: 'var(--primary)',
            fontWeight: 600,
            textDecoration: 'none',
            transition: 'color var(--transition-duration) ease',
            '&:hover': {
              color: 'color-mix(in srgb, var(--primary) 78%, var(--foreground))',
              textDecoration: 'none',
            },
          },
        },
      },
    },
  })
}
