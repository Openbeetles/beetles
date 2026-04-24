import { createContext } from 'react'

export interface UnsavedContextValue {
  dirty: boolean
  setDirty: (value: boolean) => void
  setDirtyFor: (owner: string, dirty: boolean) => void
  clearDirtyFor: (owner: string) => void
  clearAllDirty: () => void
}

export const UnsavedContext = createContext<UnsavedContextValue>({
  dirty: false,
  setDirty: () => {},
  setDirtyFor: () => {},
  clearDirtyFor: () => {},
  clearAllDirty: () => {},
})
