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

interface ToastState {
  id: number
  open: boolean
  message: string
  variant: ToastVariant
  position: ToastPosition
  autoHideDuration: number
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toast, setToast] = useState<ToastState>({
    id: 0,
    open: false,
    message: '',
    variant: 'success',
    position: 'top-center',
    autoHideDuration: 3000,
  })

  const showToast = useCallback((msg: string, options?: ToastOptions) => {
    setToast((prev) => ({
      id: prev.id + 1,
      open: true,
      message: msg,
      variant: options?.variant ?? 'success',
      position: options?.position ?? 'top-center',
      autoHideDuration: options?.autoHideDuration ?? 3000,
    }))
  }, [])

  const value = { showToast }

  const style = VARIANT_STYLES[toast.variant]

  return (
    <ToastContext.Provider value={value}>
      {children}
      <Snackbar
        key={toast.id}
        open={toast.open}
        onClose={(_, reason) => {
          if (reason === 'clickaway') return
          setToast((prev) => ({ ...prev, open: false }))
        }}
        autoHideDuration={toast.autoHideDuration}
        anchorOrigin={POSITION_MAP[toast.position]}
        message={toast.message}
        slotProps={{
          content: {
            role: toast.variant === 'error' ? 'alert' : 'status',
            'aria-live': toast.variant === 'error' ? 'assertive' : 'polite',
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
