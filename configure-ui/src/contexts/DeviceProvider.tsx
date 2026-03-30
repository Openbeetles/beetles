import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { getPairingCode } from '../api/endpoints/pairingCode'
import { fetchCsrfToken } from '../api/client'
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

export function DeviceProvider({ children }: { children: React.ReactNode }) {
  const [baseUrl, setBaseUrlState] = useState(getStoredBaseUrl)
  const [pairingCode, setPairingCodeState] = useState(getStoredPairingCode)
  const pollTimerRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const setBaseUrl = useCallback((v: string) => {
    setBaseUrlState(v)
    setStoredBaseUrl(v)
  }, [])

  const setPairingCode = useCallback((v: string) => {
    setPairingCodeState(v)
    setStoredPairingCode(v)
  }, [])

  // 初次或 baseUrl 变化时检测一次
  useEffect(() => {
    resetDeviceRuntimeKind()
    const url = baseUrl?.trim()
    if (!url) {
      setDeviceStatus('none', null)
      return
    }
    setDeviceStatus('checking', null)
    let cancelled = false
    getPairingCode(url).then((res) => {
      if (cancelled) return
      if (res.ok && res.data != null) {
        setDeviceStatus('reachable', res.data.code_set)
        updateRestartState('reachable')
        fetchCsrfToken(url)
      } else {
        setDeviceStatus('unreachable', null)
        updateRestartState('unreachable')
      }
    })
    return () => {
      cancelled = true
    }
  }, [baseUrl])

  // 有 baseUrl 时定时复检连接，便于设备断线后更新侧栏/横幅
  useEffect(() => {
    const url = baseUrl?.trim()
    if (!url) return
    const tick = () => {
      getPairingCode(url).then((res) => {
        if (res.ok && res.data != null) {
          setDeviceStatus('reachable', res.data.code_set)
          updateRestartState('reachable')
          fetchCsrfToken(url)
        } else {
          setDeviceStatus('unreachable', null)
          updateRestartState('unreachable')
        }
      })
    }
    pollTimerRef.current = setInterval(tick, CONNECTION_POLL_INTERVAL_MS)
    return () => {
      if (pollTimerRef.current) {
        clearInterval(pollTimerRef.current)
        pollTimerRef.current = null
      }
    }
  }, [baseUrl])

  const value = useMemo(
    () => ({ baseUrl, pairingCode, setBaseUrl, setPairingCode }),
    [baseUrl, pairingCode, setBaseUrl, setPairingCode],
  )

  return <DeviceContext.Provider value={value}>{children}</DeviceContext.Provider>
}
