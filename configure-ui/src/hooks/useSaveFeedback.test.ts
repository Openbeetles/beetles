import assert from 'node:assert/strict'
import test from 'node:test'
import { mapSaveFeedbackError } from './useSaveFeedback.ts'

function t(key: string): string {
  if (key === 'http.route_worker_busy') {
    return '设备正在处理另一个请求，请稍后重试。'
  }
  if (key === 'http.route_worker_memory_low') {
    return '设备当前内存余量不足，暂时无法启动该配置任务。'
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
})

test('mapSaveFeedbackError leaves prose errors untouched', () => {
  assert.equal(mapSaveFeedbackError('raw upstream failure', t), 'raw upstream failure')
})

test('mapSaveFeedbackError falls back for empty errors', () => {
  assert.equal(mapSaveFeedbackError(undefined, t), '')
})
