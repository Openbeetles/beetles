import { useCallback, useState } from 'react'
import Snackbar from '@mui/material/Snackbar'
import type { ReactNode } from 'react'
import type { ToastOptions, ToastPosition, ToastVariant } from './ToastContext'
import { ToastContext } from './ToastContext'

const POSITION_MAP: Record<ToastPosition, { vertical: 'top'; horizontal: 'center' | 'left' | 'right' }> = {
  'top-center': { vertical: 'top', horizontal: 'center' },
  'top-right': { vertical: 'top', horizontal: 'right' },
  'top-left': { vertical: 'top', horizontal: 'left' },
}

const VARIANT_STYLES: Record<ToastVariant, { bg: string; border: string; color: string }> = {
  success: {
    bg: 'color-mix(in srgb, var(--semantic-success) 8%, var(--surface))',
    border: 'color-mix(in srgb, var(--semantic-success) 22%, transparent)',
    color: 'var(--semantic-success)',
  },
  warning: {
    bg: 'color-mix(in srgb, var(--semantic-warning) 8%, var(--surface))',
    border: 'color-mix(in srgb, var(--semantic-warning) 22%, transparent)',
    color: 'var(--semantic-warning)',
  },
  error: {
    bg: 'color-mix(in srgb, var(--semantic-danger) 8%, var(--surface))',
    border: 'color-mix(in srgb, var(--semantic-danger) 22%, transparent)',
    color: 'var(--semantic-danger)',
  },
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(false)
  const [message, setMessage] = useState('')
  const [variant, setVariant] = useState<ToastVariant>('success')
  const [position, setPosition] = useState<ToastPosition>('top-center')
  const [autoHideDuration, setAutoHideDuration] = useState(3000)

  const showToast = useCallback((msg: string, options?: ToastOptions) => {
    setMessage(msg)
    setVariant(options?.variant ?? 'success')
    setPosition(options?.position ?? 'top-center')
    setAutoHideDuration(options?.autoHideDuration ?? 3000)
    setOpen(true)
  }, [])

  const value = { showToast }

  const style = VARIANT_STYLES[variant]

  return (
    <ToastContext.Provider value={value}>
      {children}
      <Snackbar
        open={open}
        onClose={() => setOpen(false)}
        autoHideDuration={autoHideDuration}
        anchorOrigin={POSITION_MAP[position]}
        message={message}
        slotProps={{
          content: {
            role: variant === 'error' ? 'alert' : 'status',
            'aria-live': variant === 'error' ? 'assertive' : 'polite',
          },
        }}
        sx={{
          '& .MuiSnackbarContent-root': {
            backgroundColor: style.bg,
            borderColor: style.border,
            color: style.color,
          },
        }}
      />
    </ToastContext.Provider>
  )
}
