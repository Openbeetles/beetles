import { useEffect } from 'react'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import { OS_ICON_DIALOG } from '../../config/osIcons'
import { Os3dIcon } from '../Os3dIcon'

type Status = 'ok' | 'fail'

interface SaveFeedbackProps {
  status: Status
  message: string
  /** 成功时多少毫秒后自动消失，0 不自动消失 */
  autoDismissMs?: number
  onDismiss?: () => void
  /**
   * `form`：表单项下方（默认上边距）。
   * `belowTitle`：`SettingsSection` 的 `belowTitleRow` 内，全宽、顶栏无额外 margin，与保存按钮仍同区块、但不塞进标题行 flex。
   */
  placement?: 'form' | 'belowTitle'
}

export function SaveFeedback({
  status,
  message,
  autoDismissMs = 3000,
  onDismiss,
  placement = 'form',
}: SaveFeedbackProps) {
  const isOk = status === 'ok'
  const isBelowTitle = placement === 'belowTitle'

  useEffect(() => {
    if (!isOk || autoDismissMs <= 0 || !onDismiss) return
    const id = setTimeout(onDismiss, autoDismissMs)
    return () => clearTimeout(id)
  }, [isOk, autoDismissMs, onDismiss])

  return (
    <Box
      role="status"
      aria-live="polite"
      aria-atomic
      sx={{
        display: 'flex',
        alignItems: isBelowTitle ? 'flex-start' : 'center',
        gap: 1,
        mt: isBelowTitle ? 0 : 2,
        width: isBelowTitle ? '100%' : undefined,
        minWidth: 0,
        p: 1.5,
        borderRadius: 'var(--radius-control)',
        bgcolor: isOk
          ? 'color-mix(in srgb, var(--semantic-success) 6%, transparent)'
          : 'color-mix(in srgb, var(--semantic-danger) 6%, transparent)',
        border: isOk
          ? '1px solid color-mix(in srgb, var(--semantic-success) 22%, transparent)'
          : '1px solid color-mix(in srgb, var(--semantic-danger) 20%, transparent)',
        boxShadow: 'var(--os3d-alert-strip-stack)',
        transition: 'opacity var(--transition-duration) ease',
      }}
    >
      <Box sx={{ width: 28, height: 28, flexShrink: 0 }}>
        <Os3dIcon
          src={isOk ? OS_ICON_DIALOG.success : OS_ICON_DIALOG.error}
          variant="inline"
        />
      </Box>
      <Typography
        variant="body2"
        sx={{
          color: isOk ? 'var(--semantic-success)' : 'var(--semantic-danger)',
          fontWeight: 500,
          flex: isBelowTitle ? 1 : undefined,
          minWidth: 0,
          wordBreak: 'break-word',
        }}
      >
        {message}
      </Typography>
    </Box>
  )
}
