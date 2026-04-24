import { useCallback, useMemo, useState } from 'react'
import { UnsavedContext } from './UnsavedContext'
import {
  clearDirtyOwner,
  clearDirtyOwners,
  isDirtyOwners,
  setDirtyOwner,
  type DirtyOwners,
} from './unsavedDirtyModel'

const LEGACY_DIRTY_OWNER = 'legacy'

export function UnsavedProvider({ children }: { children: React.ReactNode }) {
  const [dirtyOwners, setDirtyOwners] = useState<DirtyOwners>(
    clearDirtyOwners,
  )

  const setDirtyFor = useCallback((owner: string, dirty: boolean) => {
    setDirtyOwners((prev) => setDirtyOwner(prev, owner, dirty))
  }, [])

  const clearDirtyFor = useCallback((owner: string) => {
    setDirtyOwners((prev) => clearDirtyOwner(prev, owner))
  }, [])

  const clearAllDirty = useCallback(() => {
    setDirtyOwners(clearDirtyOwners())
  }, [])

  const setDirty = useCallback(
    (v: boolean) => {
      if (v) {
        setDirtyFor(LEGACY_DIRTY_OWNER, true)
      } else {
        clearAllDirty()
      }
    },
    [clearAllDirty, setDirtyFor],
  )

  const value = useMemo(
    () => ({
      dirty: isDirtyOwners(dirtyOwners),
      setDirty,
      setDirtyFor,
      clearDirtyFor,
      clearAllDirty,
    }),
    [clearAllDirty, clearDirtyFor, dirtyOwners, setDirty, setDirtyFor],
  )

  return (
    <UnsavedContext.Provider value={value}>
      {children}
    </UnsavedContext.Provider>
  )
}
