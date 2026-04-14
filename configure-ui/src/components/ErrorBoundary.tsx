import { Component, type ReactNode } from 'react'
import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import Typography from '@mui/material/Typography'
import ErrorOutline from '@mui/icons-material/ErrorOutline'
import { useTranslation } from 'react-i18next'

interface ErrorBoundaryProps {
  children: ReactNode
}

interface ErrorBoundaryState {
  hasError: boolean
  error: Error | null
}

export class ErrorBoundaryClass extends Component<
  ErrorBoundaryProps & { t: (k: string) => string },
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { hasError: false, error: null }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error }
  }

  componentDidCatch(error: Error) {
    console.error('ErrorBoundary caught:', error)
  }

  retry = () => {
    this.setState({ hasError: false, error: null })
  }

  render() {
    if (this.state.hasError) {
      const { t } = this.props
      return (
        <Box
          sx={{
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            justifyContent: 'center',
            minHeight: '60vh',
            gap: 2,
            px: 2,
          }}
        >
          <ErrorOutline
            sx={{ fontSize: 48, color: 'var(--semantic-danger)' }}
            aria-hidden
          />
          <Typography
            variant="h6"
            sx={{
              color: 'var(--foreground)',
              fontWeight: 600,
              textAlign: 'center',
            }}
          >
            {t('common.errorBoundaryTitle')}
          </Typography>
          <Typography
            variant="body2"
            sx={{ color: 'var(--text-tertiary)', textAlign: 'center', maxWidth: 360 }}
          >
            {t('common.errorBoundaryDesc')}
          </Typography>
          <Button
            variant="contained"
            onClick={this.retry}
            sx={{ borderRadius: 'var(--radius-control)' }}
          >
            {t('common.retry')}
          </Button>
        </Box>
      )
    }
    return this.props.children
  }
}

export function ErrorBoundary({ children }: ErrorBoundaryProps) {
  const { t } = useTranslation()
  return (
    <ErrorBoundaryClass t={t}>
      {children}
    </ErrorBoundaryClass>
  )
}
