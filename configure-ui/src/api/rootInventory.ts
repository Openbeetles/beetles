export interface RootInventory {
  endpoints: string[]
  windowed_endpoints: string[]
}

function normalizeStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return []
  return value.filter((item): item is string => typeof item === 'string' && item.trim().length > 0)
}

export function parseRootInventory(value: unknown): RootInventory | null {
  if (typeof value !== 'object' || value === null) return null
  const obj = value as Record<string, unknown>
  return {
    endpoints: normalizeStringArray(obj.endpoints),
    windowed_endpoints: normalizeStringArray(obj.windowed_endpoints),
  }
}

export function endpointSupportedByInventory(
  inventory: RootInventory | null,
  endpoint: string,
): boolean {
  if (!inventory) return false
  return (
    inventory.endpoints.includes(endpoint) || inventory.windowed_endpoints.includes(endpoint)
  )
}
