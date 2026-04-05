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

const R = LAYOUT_TOKENS.radiusControl
const R_CARD = LAYOUT_TOKENS.radiusCard
const R_CHIP = LAYOUT_TOKENS.radiusChip

/** 卡片内图片 hover 缩放样式，供列表/卡片等复用（动效时长来自 LAYOUT_TOKENS） */
export const cardImageHoverSx = {
  '& img': {
    transition: `transform ${LAYOUT_TOKENS.durationImageHoverMs}ms var(--ease-out-smooth)`,
  },
  '&:hover img': {
    transform: 'scale(1.04)',
  },
} as const

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
      fontSize: 15,
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
          },
          '*:focus-visible': {
            outline: `${LAYOUT_TOKENS.focusRingWidth}px solid color-mix(in srgb, var(--primary) 55%, transparent)`,
            outlineOffset: LAYOUT_TOKENS.focusRingOffset,
          },
        },
      },
      MuiButton: {
        defaultProps: { disableElevation: true },
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
            fontSize: '0.8125rem',
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
            border: '1px solid color-mix(in srgb, var(--border) 18%, transparent)',
            backgroundImage: 'none',
            transition: 'border-color var(--transition-duration) ease',
          },
        },
      },
      MuiCard: {
        styleOverrides: {
          root: {
            borderRadius: R_CARD,
            border: '1px solid color-mix(in srgb, var(--border) 18%, transparent)',
            boxShadow: 'none',
            backgroundColor: 'var(--card)',
            transition: 'border-color var(--transition-duration) ease',
          },
        },
      },
      MuiTabs: {
        styleOverrides: {
          indicator: {
            height: 'var(--accent-line-height)',
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
      MuiOutlinedInput: {
        styleOverrides: {
          root: {
            borderRadius: R,
            backgroundColor: 'var(--input-idle-well)',
            transition: 'background-color var(--transition-duration) ease, box-shadow var(--transition-duration) var(--ease-out-smooth)',
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
          },
        },
      },
      MuiChip: {
        styleOverrides: {
          root: {
            borderRadius: R_CHIP,
            fontWeight: 600,
            fontSize: '0.8125rem',
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
        styleOverrides: {
          root: {
            borderRadius: R_CHIP,
            transition: 'color var(--transition-duration) ease, background-color var(--transition-duration) ease, transform var(--transition-duration) ease',
            '&:hover': {
              backgroundColor: 'color-mix(in srgb, var(--primary) 6%, transparent)',
              color: 'var(--primary)',
            },
            '&:active': { transform: 'scale(0.96)' },
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
      MuiTooltip: {
        defaultProps: { arrow: true },
        styleOverrides: {
          tooltip: {
            backgroundColor: 'var(--foreground)',
            color: 'var(--background)',
            fontSize: '0.75rem',
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
            fontSize: '0.875rem',
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
