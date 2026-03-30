import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { getPairingCode } from '../api/endpoints/pairingCode'
import { fetchCsrfToken } from '../api/client'
import { resetSystemInfoCache } from '../session/systemInfoCoordinator'
import {
  resetDeviceRuntimeKind,
  setDeviceStatus,
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

type PollMeta = { prevConnection: 'checking' | 'reachable' | 'unreachable' | 'none'; csrfPrimed: boolean }

export function DeviceProvider({ children }: { children: React.ReactNode }) {
  const [baseUrl, setBaseUrlState] = useState(getStoredBaseUrl)
  const [pairingCode, setPairingCodeState] = useState(getStoredPairingCode)
  const pollTimerRef = useRef<ReturnType<typeof setInterval> | null>(null)
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
    resetSystemInfoCache()
  }, [baseUrl, pairingCode])

  const applyPairingResult = useCallback((url: string, cancelled: boolean, res: Awaited<ReturnType<typeof getPairingCode>>) => {
    if (cancelled) return
    const meta = pollMetaRef.current
    if (res.ok && res.data != null) {
      const wasUnreachable = meta.prevConnection === 'unreachable'
      const needCsrf = !meta.csrfPrimed || wasUnreachable
      if (needCsrf) void fetchCsrfToken(url)
      pollMetaRef.current = { prevConnection: 'reachable', csrfPrimed: true }
      setDeviceStatus('reachable', res.data.code_set)
      updateRestartState('reachable')
    } else {
      pollMetaRef.current = { ...pollMetaRef.current, prevConnection: 'unreachable' }
      setDeviceStatus('unreachable', null)
      updateRestartState('unreachable')
    }
  }, [])

  // 初次或 baseUrl 变化时检测一次
  useEffect(() => {
    resetDeviceRuntimeKind()
    const url = baseUrl?.trim()
    if (!url) {
      pollMetaRef.current = { prevConnection: 'none', csrfPrimed: false }
      setDeviceStatus('none', null)
      return
    }
    pollMetaRef.current = { prevConnection: 'checking', csrfPrimed: false }
    setDeviceStatus('checking', null)
    let cancelled = false
    getPairingCode(url).then((res) => {
      applyPairingResult(url, cancelled, res)
    })
    return () => {
      cancelled = true
    }
  }, [baseUrl, applyPairingResult])

  // 有 baseUrl 时定时复检连接，便于设备断线后更新侧栏/横幅
  useEffect(() => {
    const url = baseUrl?.trim()
    if (!url) return
    const tick = () => {
      getPairingCode(url).then((res) => {
        applyPairingResult(url, false, res)
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
