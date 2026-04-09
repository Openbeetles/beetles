import test from 'node:test'
import assert from 'node:assert/strict'
import { runDeferredLoading } from './deferredLoading.ts'

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    globalThis.setTimeout(resolve, ms)
  })
}

function createDeferred<T>(): {
  promise: Promise<T>
  resolve: (value: T) => void
} {
  let resolveFn: ((value: T) => void) | null = null
  const promise = new Promise<T>((resolve) => {
    resolveFn = resolve
  })
  if (resolveFn == null) {
    throw new Error('deferred resolver missing')
  }
  return { promise, resolve: resolveFn }
}

test('runDeferredLoading does not flip loading on after an immediate success', async () => {
  let loadingStarts = 0
  let result = ''

  runDeferredLoading({
    run: async () => 'cached',
    onStart: () => {
      loadingStarts += 1
    },
    onSuccess: (value) => {
      result = value
    },
    onError: (error) => {
      throw error
    },
  })

  await sleep(10)

  assert.equal(result, 'cached')
  assert.equal(loadingStarts, 0)
})

test('runDeferredLoading shows loading before a slow success settles', async () => {
  let loadingStarts = 0
  let result = ''
  const deferred = createDeferred<string>()

  runDeferredLoading({
    run: () => deferred.promise,
    onStart: () => {
      loadingStarts += 1
    },
    onSuccess: (value) => {
      result = value
    },
    onError: (error) => {
      throw error
    },
  })

  await sleep(10)
  assert.equal(loadingStarts, 1)
  assert.equal(result, '')

  deferred.resolve('network')
  await sleep(10)

  assert.equal(result, 'network')
})
