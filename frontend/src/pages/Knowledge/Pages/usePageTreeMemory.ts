// Copyright © 2026 Jalapeno Labs

import type { Key } from 'react-aria-components'

// Core
import { useCallback, useEffect, useMemo, useState } from 'react'

// Redux
import { selectMe } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// Utility
import { z } from 'zod'

// Misc
import { PAGE_TREE_STORAGE_KEY } from '../../../constants'

const pageTreeMemorySchema = z.object({
  // The branches open, by page id.
  openIds: z.array(z.string()),
  // The panel hidden to its edge, from `lg` up; the phone's drawer always shows it.
  isCollapsed: z.boolean(),
})

type PageTreeMemory = z.infer<typeof pageTreeMemorySchema>

const NOTHING_REMEMBERED: PageTreeMemory = { openIds: [], isCollapsed: false }

export function readPageTreeMemory(storageKey: string): PageTreeMemory {
  try {
    const stored = window.localStorage.getItem(storageKey)
    if (stored === null) {
      return NOTHING_REMEMBERED
    }
    return pageTreeMemorySchema.parse(JSON.parse(stored))
  }
  catch (error) {
    // Storage can be unavailable (privacy modes) or hold something older; the tree still works.
    console.debug('The page tree\'s state could not be read from localStorage', { error })
    return NOTHING_REMEMBERED
  }
}

// How one viewer left the tree. A viewer is a person on a browser, as the sidebar's choice is, so
// the key ends in their id and two people sharing a browser each keep their own. It lives in the
// layout rather than Redux, since only the tree reads it. Every change is saved, and a change
// saved in another tab is followed here, so neither tab writes over the other's.
export function usePageTreeMemory() {
  const personId = useAppSelector(selectMe)?.user.id
  const storageKey = `${PAGE_TREE_STORAGE_KEY}.${personId}`
  const [ memory, setMemory ] = useState(() => readPageTreeMemory(storageKey))

  useEffect(() => {
    try {
      window.localStorage.setItem(storageKey, JSON.stringify(memory))
    }
    catch (error) {
      console.debug('The page tree\'s state could not be saved to localStorage', { error })
    }
  }, [ storageKey, memory ])

  useEffect(() => {
    function followOtherTab(event: StorageEvent) {
      if (event.key !== storageKey) {
        return
      }
      setMemory(readPageTreeMemory(storageKey))
    }

    window.addEventListener('storage', followOtherTab)
    return () => window.removeEventListener('storage', followOtherTab)
  }, [ storageKey ])

  const openIds = useMemo(() => new Set(memory.openIds), [ memory.openIds ])

  const setOpenIds = useCallback((ids: Set<Key>) => {
    setMemory((current) => ({ ...current, openIds: Array.from(ids, String) }))
  }, [])

  // Opens these branches and leaves the rest as they are.
  const openBranches = useCallback((ids: string[]) => {
    setMemory((current) => {
      const closedIds = ids.filter((id) => !current.openIds.includes(id))
      if (!closedIds.length) {
        return current
      }
      return { ...current, openIds: [ ...current.openIds, ...closedIds ]}
    })
  }, [])

  const setIsCollapsed = useCallback((isCollapsed: boolean) => {
    setMemory((current) => ({ ...current, isCollapsed }))
  }, [])

  return {
    openIds,
    setOpenIds,
    openBranches,
    isCollapsed: memory.isCollapsed,
    setIsCollapsed,
  } as const
}

export type PageTreeMemoryState = ReturnType<typeof usePageTreeMemory>
