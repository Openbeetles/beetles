import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'

interface PageHeaderProps {
  title: string
  brandLabel?: string
}

/**
 * 顶栏标题行（与 `TopBar` 窗口隐喻一致）；不再展示副标题，说明由各页正文承担。
 * Title row for shell titlebar; no subtitle — page body carries context.
 */
export function PageHeader({ title, brandLabel }: PageHeaderProps) {
  return (
    <Box
      sx={{
        py: 0.125,
        display: 'flex',
        alignItems: 'center',
        gap: 0.875,
        minWidth: 0,
      }}
    >
      {brandLabel ? (
        <Box
          component="span"
          sx={{
            flexShrink: 0,
            display: 'inline-flex',
            alignItems: 'center',
            px: 1,
            py: 0.45,
            borderRadius: 'calc(var(--radius-chip) - 1px)',
            border:
              '1px solid color-mix(in srgb, var(--primary) 14%, var(--border))',
            backgroundColor:
              'color-mix(in srgb, var(--primary) 7%, var(--card))',
            backgroundImage:
              'linear-gradient(180deg, color-mix(in srgb, #fff 12%, transparent) 0%, transparent 100%)',
            boxShadow: 'var(--os3d-control-soft-lift-stack)',
          }}
        >
          <Typography
            component="span"
            sx={{
              fontFamily: 'var(--font-display)',
              fontSize: '0.625rem',
              fontWeight: 400,
              letterSpacing: '0.08em',
              lineHeight: 1,
              textTransform: 'uppercase',
              color: 'var(--text-secondary)',
              whiteSpace: 'nowrap',
            }}
          >
            {brandLabel}
          </Typography>
        </Box>
      ) : null}
      <Typography
        component="h1"
        sx={{
          flex: 1,
          minWidth: 0,
          fontFamily: 'var(--font-sans)',
          fontSize: 'clamp(1rem, 0.96rem + 0.18vw, 1.125rem)',
          fontWeight: 700,
          letterSpacing: '-0.01em',
          lineHeight: 1.2,
          color: 'var(--text-primary)',
          margin: 0,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
        {title}
      </Typography>
    </Box>
  )
}
