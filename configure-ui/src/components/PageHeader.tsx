import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'

interface PageHeaderProps {
  title: string
}

/**
 * 顶栏标题行（与 `TopBar` 窗口隐喻一致）；不再展示副标题，说明由各页正文承担。
 * Title row for shell titlebar; no subtitle — page body carries context.
 */
export function PageHeader({ title }: PageHeaderProps) {
  return (
    <Box
      sx={{
        py: 0.5,
        flex: 1,
        minWidth: 0,
        position: 'relative',
      }}
    >
      <Typography
        component="h1"
        sx={{
          fontFamily: 'var(--font-sans)',
          fontSize: 'var(--font-size-body-lg)',
          fontWeight: 600,
          letterSpacing: '-0.02em',
          lineHeight: 'var(--line-height-snug)',
          color: 'var(--foreground)',
          margin: 0,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
          WebkitFontSmoothing: 'antialiased',
          MozOsxFontSmoothing: 'grayscale',
        }}
      >
        {title}
      </Typography>
      <Box
        aria-hidden
        sx={{
          mt: 0.75,
          width: 32,
          height: 3,
          borderRadius: 'var(--radius-full)',
          background:
            'linear-gradient(90deg, color-mix(in srgb, var(--primary) 34%, transparent), color-mix(in srgb, var(--primary) 8%, transparent))',
          boxShadow:
            'inset 0 1px 0 color-mix(in srgb, #fff 34%, transparent), 0 2px 6px -4px color-mix(in srgb, var(--primary) 22%, transparent)',
        }}
      />
    </Box>
  )
}
