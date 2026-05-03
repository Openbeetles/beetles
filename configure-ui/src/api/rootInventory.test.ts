import test from 'node:test'
import assert from 'node:assert/strict'
import {
  apiResultIndicatesUnsupportedEndpoint,
  endpointSupportedByInventory,
  parseRootInventory,
  type RootInventory,
} from './rootInventory.ts'

test('parseRootInventory extracts endpoint arrays from root payload', () => {
  const inventory = parseRootInventory({
    endpoints: ['GET /api/health', 'GET /api/tools'],
    windowed_endpoints: ['GET /api/skills'],
  })

  assert.deepEqual(inventory, {
    endpoints: ['GET /api/health', 'GET /api/tools'],
    windowed_endpoints: ['GET /api/skills'],
  } satisfies RootInventory)
})

test('endpointSupportedByInventory checks both always-on and windowed endpoints', () => {
  const inventory: RootInventory = {
    endpoints: ['GET /api/tools'],
    windowed_endpoints: ['GET /api/skills'],
  }

  assert.equal(endpointSupportedByInventory(inventory, 'GET /api/tools'), true)
  assert.equal(endpointSupportedByInventory(inventory, 'GET /api/skills'), true)
  assert.equal(endpointSupportedByInventory(inventory, 'POST /api/skills'), false)
})

test('apiResultIndicatesUnsupportedEndpoint accepts capability unsupported responses', () => {
  assert.equal(
    apiResultIndicatesUnsupportedEndpoint({
      status: 501,
      errorKey: 'capability.unsupported',
    }),
    true,
  )
  assert.equal(
    apiResultIndicatesUnsupportedEndpoint({
      status: 500,
      errorKey: 'common.operation_failed',
    }),
    false,
  )
})
