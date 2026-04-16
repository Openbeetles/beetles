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
        py: 0.25,
        flex: 1,
        minWidth: 0,
        position: 'relative',
      }}
    >
      <Typography
        component="h1"
        sx={{
          fontFamily: 'var(--font-sans)',
          fontSize: 'var(--font-size-body)',
          fontWeight: 600,
          letterSpacing: '-0.015em',
          lineHeight: 'var(--line-height-snug)',
          color: 'var(--text-primary)',
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
    </Box>
  )
}
