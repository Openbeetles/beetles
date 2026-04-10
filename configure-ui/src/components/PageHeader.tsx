import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'

export type PageHeaderVariant = 'page' | 'bar'

interface PageHeaderProps {
  title: string
  description?: string
  /** bar：用于 Layout 顶部栏，无下边距；page：用于独立页面（已弃用，由 Layout 统一展示） */
  variant?: PageHeaderVariant
}

export function PageHeader({ title, description, variant = 'page' }: PageHeaderProps) {
  const inBar = variant === 'bar'
  return (
    <Box
      sx={{
        ...(inBar ? { py: 1.5, flex: 1, minWidth: 0 } : { mb: 4, pb: 3 }),
        position: 'relative',
        ...(!inBar && {
          borderBottom: 'none',
          '&::after': {
            content: '""',
            position: 'absolute',
            left: 0,
            bottom: -1,
            width: 'var(--page-header-accent-width)',
            height: 'var(--accent-line-height)',
            borderRadius: 'var(--radius-chip)',
            background: 'linear-gradient(90deg, color-mix(in srgb, var(--primary) 35%, transparent), transparent)',
            opacity: 0.7,
          },
        }),
      }}
    >
      <Typography
        component="h1"
        sx={{
          /** 顶栏：系统菜单栏式标题；独立页：保留品牌 Display 字重 */
          fontFamily: inBar ? 'var(--font-sans)' : 'var(--font-display)',
          fontSize: inBar ? 'var(--font-size-body)' : { xs: 'var(--font-size-h4)', md: 'var(--font-size-h3)' },
          fontWeight: inBar ? 600 : 700,
          letterSpacing: inBar ? '-0.01em' : 'var(--letter-spacing-tight)',
          lineHeight: 'var(--line-height-tight)',
          color: 'var(--foreground)',
          margin: 0,
          ...(inBar && { overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }),
        }}
      >
        {title}
      </Typography>
      {description != null && description !== '' && (
        <Typography
          component="p"
          sx={{
            mt: inBar ? 0.5 : 1,
            fontSize: inBar ? 'var(--font-size-caption)' : 'var(--font-size-body-sm)',
            fontWeight: 400,
            lineHeight: 'var(--line-height-normal)',
            color: inBar ? 'var(--foreground)' : 'var(--muted)',
            opacity: inBar ? 0.55 : 1,
            maxWidth: inBar ? '42rem' : '52ch',
            ...(inBar && { overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }),
          }}
        >
          {description}
        </Typography>
      )}
    </Box>
  )
}
