export interface DeferredLoadingOptions<T> {
  delayMs?: number
  run: () => Promise<T>
  onStart: () => void
  onSuccess: (value: T) => void
  onError: (error: unknown) => void
}

/**
 * Delays the visible loading transition, but cancels it if the async work
 * settles before the timer fires. This avoids "data already rendered while the
 * loading bar is still stuck on" races for cached / coalesced requests.
 */
export function runDeferredLoading<T>({
  delayMs = 0,
  run,
  onStart,
  onSuccess,
  onError,
}: DeferredLoadingOptions<T>): () => void {
  let cancelled = false
  let settled = false

  const timer = globalThis.setTimeout(() => {
    if (!cancelled && !settled) {
      onStart()
    }
  }, delayMs)

  void Promise.resolve()
    .then(run)
    .then((value) => {
      settled = true
      globalThis.clearTimeout(timer)
      if (!cancelled) {
        onSuccess(value)
      }
    })
    .catch((error: unknown) => {
      settled = true
      globalThis.clearTimeout(timer)
      if (!cancelled) {
        onError(error)
      }
    })

  return () => {
    cancelled = true
    globalThis.clearTimeout(timer)
  }
}
