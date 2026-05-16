export type PairingPollConnection = 'checking' | 'reachable' | 'unreachable' | 'none'

export interface PairingPollMeta {
  prevConnection: PairingPollConnection
  csrfPrimed: boolean
  consecutiveFailures: number
}

export interface PairingPollSuccessDecision {
  meta: PairingPollMeta
  shouldPrimeCsrf: boolean
}

export interface PairingPollFailureDecision {
  meta: PairingPollMeta
  shouldMarkUnreachable: boolean
}

export const REACHABLE_POLL_FAILURES_BEFORE_UNREACHABLE = 3

export function initialPairingPollMeta(
  prevConnection: PairingPollConnection,
  csrfPrimed = false,
): PairingPollMeta {
  return { prevConnection, csrfPrimed, consecutiveFailures: 0 }
}

export function nextPairingPollMetaOnSuccess(
  meta: PairingPollMeta,
): PairingPollSuccessDecision {
  const wasUnreachable = meta.prevConnection === 'unreachable'
  return {
    meta: { prevConnection: 'reachable', csrfPrimed: true, consecutiveFailures: 0 },
    shouldPrimeCsrf: !meta.csrfPrimed || wasUnreachable,
  }
}

export function nextPairingPollMetaOnFailure(
  meta: PairingPollMeta,
  failureThreshold = REACHABLE_POLL_FAILURES_BEFORE_UNREACHABLE,
): PairingPollFailureDecision {
  const consecutiveFailures = meta.consecutiveFailures + 1
  const shouldDebounceReachableFailure =
    meta.prevConnection === 'reachable' && consecutiveFailures < failureThreshold
  const shouldMarkUnreachable = !shouldDebounceReachableFailure

  return {
    meta: {
      prevConnection: shouldMarkUnreachable ? 'unreachable' : meta.prevConnection,
      csrfPrimed: shouldMarkUnreachable ? false : meta.csrfPrimed,
      consecutiveFailures,
    },
    shouldMarkUnreachable,
  }
}
