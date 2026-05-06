import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { getPairingCode } from '../api/endpoints/pairingCode'
import {
  buildProtectedApiSessionKey,
  clearCsrfToken,
  fetchCsrfToken,
  setProtectedApiAuthObserver,
} from '../api/client'
import { resetSystemInfoCache } from '../session/systemInfoCoordinator'
import {
  deriveLocalPairingState,
  markPairingAuthInvalid,
  markPairingAuthValid,
  resetPairingAuthState,
  setDeviceProbeState,
  setDeviceSessionState,
  resetDeviceRuntimeKind,
  shouldPreservePairingAuthOnSessionChange,
  updateRestartState,
} from '../store/deviceStatusStore'
import {
  DeviceContext,
  getStoredBaseUrl,
  getStoredPairingCode,
  setStoredBaseUrl,
  setStoredPairingCode,
} from './DeviceContext'

/** 定时检测设备连接间隔（毫秒），用于更快更新重启与连接状态 */
const CONNECTION_POLL_INTERVAL_MS = 10_000
const CONNECTION_POLL_TIMEOUT_MS = 5_000

type PollMeta = { prevConnection: 'checking' | 'reachable' | 'unreachable' | 'none'; csrfPrimed: boolean }
type DeviceSessionMeta = { baseUrl: string; pairingCode: string }

export function DeviceProvider({ children }: { children: React.ReactNode }) {
  const [baseUrl, setBaseUrlState] = useState(getStoredBaseUrl)
  const [pairingCode, setPairingCodeState] = useState(getStoredPairingCode)
  const pollTimerRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const pollGenerationRef = useRef(0)
  const previousDeviceSessionRef = useRef<DeviceSessionMeta>({
    baseUrl: (getStoredBaseUrl() ?? '').trim(),
    pairingCode: (getStoredPairingCode() ?? '').trim(),
  })
  /** 配对轮询元数据：避免每次成功都 GET /api/csrf_token；仅在换机后首次成功或 unreachable→reachable 时预热。 */
  const pollMetaRef = useRef<PollMeta>({ prevConnection: 'none', csrfPrimed: false })

  const setBaseUrl = useCallback((v: string) => {
    setBaseUrlState(v)
    setStoredBaseUrl(v)
  }, [])

  const setPairingCode = useCallback((v: string) => {
    setPairingCodeState(v)
    setStoredPairingCode(v)
  }, [])

  useEffect(() => {
    clearCsrfToken()
    resetSystemInfoCache()
    const normalizedBaseUrl = baseUrl?.trim() ?? ''
    const normalizedPairingCode = (pairingCode ?? '').trim()
    const previousDeviceSession = previousDeviceSessionRef.current
    const preserveAuth = shouldPreservePairingAuthOnSessionChange({
      previousBaseUrl: previousDeviceSession.baseUrl,
      previousPairingCode: previousDeviceSession.pairingCode,
      nextBaseUrl: normalizedBaseUrl,
      nextPairingCode: normalizedPairingCode,
    })
    previousDeviceSessionRef.current = {
      baseUrl: normalizedBaseUrl,
      pairingCode: normalizedPairingCode,
    }
    setDeviceSessionState({
      hasTarget: Boolean(normalizedBaseUrl),
      localPairing: deriveLocalPairingState(normalizedPairingCode),
      preserveAuth,
    })
  }, [baseUrl, pairingCode])

  useEffect(() => {
    const activeSessionKey = buildProtectedApiSessionKey(
      baseUrl ?? '',
      pairingCode ?? '',
    )
    setProtectedApiAuthObserver((event) => {
      if (event.sessionKey !== activeSessionKey) {
        return
      }
      if (event.state === 'valid') {
        markPairingAuthValid()
        return
      }
      if (event.state === 'invalid') {
        markPairingAuthInvalid()
        return
      }
      resetPairingAuthState()
    })
    return () => {
      setProtectedApiAuthObserver(null)
    }
  }, [baseUrl, pairingCode])

  const applyPairingResult = useCallback((
    url: string,
    generation: number,
    cancelled: boolean,
    res: Awaited<ReturnType<typeof getPairingCode>>,
  ) => {
    if (cancelled || generation !== pollGenerationRef.current) return
    const meta = pollMetaRef.current
    if (res.ok && res.data != null) {
      const wasUnreachable = meta.prevConnection === 'unreachable'
      const needCsrf = !meta.csrfPrimed || wasUnreachable
      if (needCsrf) void fetchCsrfToken(url)
      pollMetaRef.current = { prevConnection: 'reachable', csrfPrimed: true }
      setDeviceProbeState({
        transport: 'reachable',
        devicePairing: res.data.code_set ? 'initialized' : 'uninitialized',
      })
      updateRestartState('reachable')
    } else {
      pollMetaRef.current = { ...pollMetaRef.current, prevConnection: 'unreachable' }
      setDeviceProbeState({ transport: 'unreachable', devicePairing: 'unknown' })
      updateRestartState('unreachable')
    }
  }, [])

  // 初次或 baseUrl 变化时检测一次
  useEffect(() => {
    resetDeviceRuntimeKind()
    const generation = pollGenerationRef.current + 1
    pollGenerationRef.current = generation
    const url = baseUrl?.trim()
    if (!url) {
      pollMetaRef.current = { prevConnection: 'none', csrfPrimed: false }
      return
    }
    pollMetaRef.current = { prevConnection: 'checking', csrfPrimed: false }
    setDeviceProbeState({ transport: 'checking', devicePairing: 'unknown' })
    let cancelled = false
    getPairingCode(url, { timeoutMs: CONNECTION_POLL_TIMEOUT_MS }).then((res) => {
      applyPairingResult(url, generation, cancelled, res)
    })
    return () => {
      cancelled = true
    }
  }, [baseUrl, applyPairingResult])

  // 有 baseUrl 时定时复检连接，便于设备断线后更新侧栏/横幅
  useEffect(() => {
    const url = baseUrl?.trim()
    if (!url) return
    const generation = pollGenerationRef.current
    const tick = () => {
      getPairingCode(url, { timeoutMs: CONNECTION_POLL_TIMEOUT_MS }).then((res) => {
        applyPairingResult(url, generation, false, res)
      })
    }
    pollTimerRef.current = setInterval(tick, CONNECTION_POLL_INTERVAL_MS)
    return () => {
      if (pollTimerRef.current) {
        clearInterval(pollTimerRef.current)
        pollTimerRef.current = null
      }
    }
  }, [baseUrl, applyPairingResult])

  const value = useMemo(
    () => ({ baseUrl, pairingCode, setBaseUrl, setPairingCode }),
    [baseUrl, pairingCode, setBaseUrl, setPairingCode],
  )

  return <DeviceContext.Provider value={value}>{children}</DeviceContext.Provider>
}
