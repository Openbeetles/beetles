import { useCallback, useState } from 'react'
import {
  translateApiError,
  type ApiErrorTranslator,
} from '../i18n/apiErrors.ts'
import type { SaveStatus } from '../types/asyncState'

export function mapSaveFeedbackError(
  error: string | undefined,
  t: ApiErrorTranslator,
): string {
  if (!error) return ''
  return translateApiError(t, error, 'common.error')
}

export function useSaveFeedback(t: ApiErrorTranslator) {
  const [status, setStatus] = useState<SaveStatus>('idle')
  const [error, setError] = useState('')

  const begin = useCallback(() => {
    setStatus('saving')
    setError('')
  }, [])

  const fail = useCallback((message: string) => {
    setStatus('fail')
    setError(message)
  }, [])

  const finishFromResult = useCallback(
    (result: { ok: boolean; error?: string }) => {
      setStatus(result.ok ? 'ok' : 'fail')
      setError(mapSaveFeedbackError(result.error, t))
    },
    [t],
  )

  const dismiss = useCallback(() => {
    setStatus('idle')
    setError('')
  }, [])

  return { status, error, begin, fail, finishFromResult, dismiss }
}
