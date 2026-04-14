import Stack from '@mui/material/Stack'
import type { ReactNode } from 'react'
import { LAYOUT_TOKENS } from '../../config/themeTokens'

/** 表单项纵向排列，间距见 `LAYOUT_TOKENS.spacingFormFields`（与全站表单节奏一致）。 */
export function FormFieldStack({ children }: { children: ReactNode }) {
  return (
    <Stack
      spacing={LAYOUT_TOKENS.spacingFormFields}
      sx={{ '& .MuiTextField-root': { minWidth: 0 } }}
    >
      {children}
    </Stack>
  )
}
