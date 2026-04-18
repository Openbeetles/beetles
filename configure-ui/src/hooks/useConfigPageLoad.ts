import { useEffect, useRef } from 'react'

export function useConfigPageLoad(params: {
  hasConfig: boolean
  loading: boolean
  loadConfig: () => Promise<void>
  canLoad?: boolean
}) {
  const { hasConfig, loading, loadConfig, canLoad = true } = params
  const loadAttemptedRef = useRef(false)

  useEffect(() => {
    if (!canLoad) {
      loadAttemptedRef.current = false
      return
    }
    if (hasConfig) {
      loadAttemptedRef.current = false
      return
    }
    if (loading || loadAttemptedRef.current) return
    loadAttemptedRef.current = true
    loadConfig()
  }, [canLoad, hasConfig, loading, loadConfig])
}
