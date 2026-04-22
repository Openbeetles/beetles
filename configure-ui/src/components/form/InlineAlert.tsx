import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import ErrorOutline from '@mui/icons-material/ErrorOutline'
import { useTranslation } from 'react-i18next'
import { LAYOUT_TOKENS } from '../../config/themeTokens'
import {
  isDeviceOrPairingErrorKey,
  translateApiError,
} from '../../i18n/apiErrors'

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
  if (isDeviceOrPairingErrorKey(message)) return null

  const display = translateApiError(t, message, 'config.errorLoadFailed')

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
