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
        /** 顶栏：紧凑标题栏行高，与 `TopBar` 窗口隐喻一致 */
        ...(inBar ? { py: 1, flex: 1, minWidth: 0 } : { mb: 4, pb: 3 }),
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
            backgroundColor: 'color-mix(in srgb, var(--primary) 40%, transparent)',
            opacity: 0.7,
          },
        }),
      }}
    >
      <Typography
        component="h1"
        sx={{
          /** 顶栏：标题栏字重（Segoe 式半粗）；独立页：品牌 Display */
          fontFamily: inBar ? 'var(--font-sans)' : 'var(--font-display)',
          fontSize: inBar ? 'var(--font-size-body-sm)' : { xs: 'var(--font-size-h4)', md: 'var(--font-size-h3)' },
          fontWeight: inBar ? 600 : 700,
          letterSpacing: inBar ? '-0.02em' : 'var(--letter-spacing-tight)',
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
            mt: inBar ? 0.25 : 1,
            fontSize: inBar ? 'var(--font-size-caption)' : 'var(--font-size-body-sm)',
            fontWeight: 400,
            lineHeight: 'var(--line-height-normal)',
            color: 'var(--muted)',
            maxWidth: inBar ? '42rem' : '52ch',
            ...(inBar && {
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
              /** 窄屏只保留标题，说明在任务栏/页面内消化 */
              display: { xs: 'none', sm: 'block' },
            }),
          }}
        >
          {description}
        </Typography>
      )}
    </Box>
  )
}
