import assert from 'node:assert/strict'
import test from 'node:test'
import { enUS } from '../i18n/locales/en-US.ts'
import { zhCN } from '../i18n/locales/zh-CN.ts'
import { mapSaveFeedbackError } from './useSaveFeedback.ts'

function t(key: string): string {
  if (key === 'http.route_worker_busy') {
    return '设备正在处理另一个请求，请稍后重试。'
  }
  if (key === 'http.route_worker_memory_low') {
    return '设备当前内存余量不足，暂时无法启动该配置任务。'
  }
  if (key === 'runtime.transport_blocked') {
    return '设备正在保护关键通信资源，请稍后重试。'
  }
  if (key === 'common.error') {
    return '操作失败'
  }
  return key
}

test('mapSaveFeedbackError translates structured API error keys', () => {
  assert.equal(
    mapSaveFeedbackError('http.route_worker_busy', t),
    '设备正在处理另一个请求，请稍后重试。',
  )
  assert.equal(
    mapSaveFeedbackError('http.route_worker_memory_low', t),
    '设备当前内存余量不足，暂时无法启动该配置任务。',
  )
  assert.equal(
    mapSaveFeedbackError('runtime.transport_blocked', t),
    '设备正在保护关键通信资源，请稍后重试。',
  )
})

test('mapSaveFeedbackError leaves prose errors untouched', () => {
  assert.equal(mapSaveFeedbackError('raw upstream failure', t), 'raw upstream failure')
})

test('mapSaveFeedbackError falls back for empty errors', () => {
  assert.equal(mapSaveFeedbackError(undefined, t), '')
})

test('runtime transport error keys have locale entries', () => {
  assert.notEqual(zhCN.translation.runtime.transport_blocked, 'runtime.transport_blocked')
  assert.notEqual(enUS.translation.runtime.transport_blocked, 'runtime.transport_blocked')
  assert.notEqual(
    zhCN.translation.runtime.route_blocked_by_config_active,
    'runtime.route_blocked_by_config_active',
  )
  assert.notEqual(
    enUS.translation.runtime.route_blocked_by_recovery_safe_mode,
    'runtime.route_blocked_by_recovery_safe_mode',
  )
})
