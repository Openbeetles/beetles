import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import Typography from '@mui/material/Typography'
import ErrorOutline from '@mui/icons-material/ErrorOutline'
import { useTranslation } from 'react-i18next'

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
}

/** 是否为 i18n key（ConfigProvider 等传入） */
function isI18nKey(msg: string): boolean {
  return /^[a-z]+\.[a-zA-Z0-9.]+$/.test(msg.trim())
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
  const display = isI18nKey(trimmed)
    ? t(trimmed)
    : ERROR_TO_I18N[trimmed]
      ? t(ERROR_TO_I18N[trimmed])
      : message

  return (
    <Box
      role="alert"
      sx={{
        display: 'flex',
        flexWrap: 'wrap',
        alignItems: 'flex-start',
        gap: 1.5,
        px: 2,
        py: 1.5,
        borderRadius: 'var(--radius-control)',
        border: '1px solid color-mix(in srgb, var(--semantic-danger) 22%, transparent)',
        borderLeft: 'var(--accent-line-width) solid var(--semantic-danger)',
        backgroundColor: 'color-mix(in srgb, var(--semantic-danger) 5%, var(--surface))',
        boxShadow: 'var(--os3d-alert-strip-stack)',
      }}
    >
      <ErrorOutline
        sx={{ fontSize: 'var(--icon-size-md)', color: 'var(--semantic-danger)', flexShrink: 0, mt: 0.25 }}
        aria-hidden
      />
      <Typography
        variant="body2"
        component="span"
        sx={{
          flex: '1 1 auto',
          minWidth: 0,
          color: 'var(--semantic-danger)',
          fontWeight: 500,
          fontSize: 'var(--font-size-body-sm)',
        }}
      >
        {display}
      </Typography>
      {onRetry && (
        <Button
          size="small"
          variant="outlined"
          onClick={onRetry}
          sx={{ flexShrink: 0 }}
        >
          {t('common.retry')}
        </Button>
      )}
    </Box>
  )
}
