import Stack from '@mui/material/Stack'
import type { ReactNode } from 'react'

/** 表单项纵向排列，统一间距（略大于默认以保留 OS 式呼吸感）。 */
export function FormFieldStack({ children }: { children: ReactNode }) {
  return (
    <Stack spacing={2.5} sx={{ '& .MuiTextField-root': { minWidth: 0 } }}>
      {children}
    </Stack>
  )
}
