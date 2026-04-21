import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import ErrorOutline from '@mui/icons-material/ErrorOutline'
import { useTranslation } from 'react-i18next'
import { LAYOUT_TOKENS } from '../../config/themeTokens'

/** 与顶栏横幅重复的配对/设备类错误，此处不重复展示 */
const DEVICE_PAIRING_HINTS = new Set([
  '请先设置配对码', '请先填写设备地址', '配对码错误',
  'Please set pairing code first', 'Please enter device URL', 'Wrong pairing code',
])

/** 已知 API/ 前端错误文案 -> i18n key（仅用于加载/数据错误） */
const ERROR_TO_I18N: Record<string, string> = {
  '加载配置失败': 'config.errorLoadFailed',
  'Load failed': 'config.errorLoadFailed',
  'Network error': 'config.errorNetwork',
  'Failed to fetch': 'config.errorNetwork',
}

/** 是否为 i18n key（ConfigProvider 等传入） */
function isI18nKey(msg: string): boolean {
  return /^[a-z]+\.[a-zA-Z0-9.]+$/.test(msg.trim())
}

function inferErrorKey(msg: string): string | null {
  const trimmed = msg.trim()
  if (ERROR_TO_I18N[trimmed]) return ERROR_TO_I18N[trimmed]

  const normalized = trimmed.toLowerCase()
  if (
    normalized === 'failed to fetch' ||
    normalized.endsWith('failed to fetch') ||
    normalized.includes('fetch failed') ||
    normalized.includes('network request failed') ||
    normalized.includes('networkerror')
  ) {
    return 'config.errorNetwork'
  }
  if (normalized.includes('operator window required')) {
    return 'config.errorLoadFailed'
  }

  return null
}

interface InlineAlertProps {
  /** 原始错误文案（仅用于页面加载/数据错误，勿传按钮操作错误） */
  message: string | null
  /** 加载失败时展示重试按钮，点击后调用（如 loadConfig） */
  onRetry?: () => void
}

/**
 * 子页加载/数据错误统一展示：与顶栏横幅、SaveFeedback 风格一致；设备/配对类不重复展示。
 */
export function InlineAlert({ message, onRetry }: InlineAlertProps) {
  const { t } = useTranslation()

  if (!message?.trim()) return null
  if (DEVICE_PAIRING_HINTS.has(message.trim())) return null

  const trimmed = message.trim()
  const resolvedKey = isI18nKey(trimmed) ? trimmed : inferErrorKey(trimmed)
  const display = resolvedKey ? t(resolvedKey) : message

  return (
    <Box
      role="alert"
      aria-live="polite"
      aria-atomic="true"
      sx={{
        display: 'flex',
        flexWrap: 'wrap',
        alignItems: 'flex-start',
        gap: 1.5,
        px: 2,
        py: 1.35,
        borderRadius: 'var(--radius-control)',
        border:
          '1px solid color-mix(in srgb, var(--semantic-danger) 12%, var(--border))',
        backgroundColor:
          'color-mix(in srgb, var(--card) 74%, transparent)',
        backgroundImage: [
          'linear-gradient(180deg, color-mix(in srgb, #fff 28%, transparent) 0%, color-mix(in srgb, #fff 8%, transparent) 46%, transparent 100%)',
          'linear-gradient(135deg, color-mix(in srgb, var(--semantic-danger) 5%, transparent) 0%, transparent 38%, color-mix(in srgb, var(--accent) 5%, transparent) 100%)',
        ].join(', '),
        boxShadow: [
          '0 18px 34px -28px color-mix(in srgb, var(--semantic-danger) 18%, transparent)',
          '0 10px 26px -24px color-mix(in srgb, var(--foreground) 14%, transparent)',
          'inset 0 1px 0 color-mix(in srgb, #fff 55%, transparent)',
        ].join(', '),
        backdropFilter: 'blur(calc(var(--glass-blur) * 0.72)) saturate(1.08)',
        WebkitBackdropFilter: 'blur(calc(var(--glass-blur) * 0.72)) saturate(1.08)',
      }}
    >
      <Stack
        direction="row"
        spacing={1.5}
        alignItems="flex-start"
        sx={{ flex: '1 1 320px', minWidth: 0, pt: 0.1 }}
      >
        <Box
          sx={{
            width: 34,
            height: 34,
            borderRadius: '999px',
            flexShrink: 0,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            border:
              '1px solid color-mix(in srgb, var(--semantic-danger) 16%, transparent)',
            backgroundColor:
              'color-mix(in srgb, #fff 30%, transparent)',
            boxShadow:
              'inset 0 1px 0 color-mix(in srgb, #fff 56%, transparent)',
          }}
        >
          <ErrorOutline
            sx={{
              fontSize: '1.05rem',
              color:
                'color-mix(in srgb, var(--semantic-danger) 82%, var(--foreground))',
            }}
            aria-hidden
          />
        </Box>
        <Box sx={{ flex: 1, minWidth: 0 }}>
          <Typography
            variant="body2"
            component="p"
            sx={{
              m: 0,
              color:
                'color-mix(in srgb, var(--semantic-danger) 32%, var(--foreground))',
              fontWeight: 700,
              fontSize: 'var(--font-size-body)',
              lineHeight: 'var(--line-height-snug)',
            }}
          >
            {t('common.inlineErrorTitle')}
          </Typography>
          <Typography
            variant="body2"
            component="p"
            sx={{
              m: 0,
              mt: 0.35,
              color: 'var(--text-secondary)',
              fontSize: 'var(--font-size-body-sm)',
              lineHeight: 'var(--line-height-relaxed)',
              wordBreak: 'break-word',
            }}
          >
            {display}
          </Typography>
        </Box>
      </Stack>
      {onRetry && (
        <Button
          size="small"
          variant="outlined"
          onClick={onRetry}
          sx={{
            flexShrink: 0,
            ml: 'auto',
            alignSelf: 'center',
            px: 1.5,
            borderRadius: `${LAYOUT_TOKENS.radiusSearchPill}px`,
            color:
              'color-mix(in srgb, var(--semantic-danger) 54%, var(--foreground))',
            borderColor:
              'color-mix(in srgb, var(--semantic-danger) 18%, var(--border))',
            backgroundColor:
              'color-mix(in srgb, var(--card) 60%, transparent)',
            boxShadow: [
              '0 10px 24px -24px color-mix(in srgb, var(--semantic-danger) 22%, transparent)',
              'inset 0 1px 0 color-mix(in srgb, #fff 48%, transparent)',
            ].join(', '),
            backdropFilter: 'blur(calc(var(--glass-blur) * 0.55))',
            WebkitBackdropFilter: 'blur(calc(var(--glass-blur) * 0.55))',
            '&:hover': {
              borderColor:
                'color-mix(in srgb, var(--semantic-danger) 24%, var(--border))',
              backgroundColor:
                'color-mix(in srgb, var(--semantic-danger) 6%, transparent)',
            },
          }}
        >
          {t('common.retry')}
        </Button>
      )}
    </Box>
  )
}
