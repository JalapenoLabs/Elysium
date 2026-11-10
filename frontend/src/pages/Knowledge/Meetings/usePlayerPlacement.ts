// Copyright © 2026 Jalapeno Labs

import type { PlayerPlacement } from './meetingPlayer'

// Core
import { useCallback, useEffect, useState } from 'react'

// Redux
import { selectMe } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// Utility
import { z } from 'zod'

// Misc
import { MEETING_PLAYER_STORAGE_KEY } from '../../../constants'
import { DEFAULT_PLAYER_PLACEMENT } from './meetingPlayer'

const placementSchema = z.object({
  x: z.number().min(0).max(1),
  y: z.number().min(0).max(1),
})

export function readPlayerPlacement(storageKey: string): PlayerPlacement {
  try {
    const stored = window.localStorage.getItem(storageKey)
    if (stored === null) {
      return DEFAULT_PLAYER_PLACEMENT
    }
    return placementSchema.parse(JSON.parse(stored))
  }
  catch (error) {
    // Storage can be unavailable (privacy modes) or hold something older; the player floats in
    // its corner all the same.
    console.debug('The meeting player\'s place could not be read from localStorage', { error })
    return DEFAULT_PLAYER_PLACEMENT
  }
}

// Where one viewer left a meeting's floating player, the same for every meeting. A viewer is a
// person on a browser, as the page tree's memory is, so the key ends in their id. A drag moves it
// as it goes and is saved once it ends (`save`), and a place saved in another tab is followed.
export function usePlayerPlacement() {
  const personId = useAppSelector(selectMe)?.user.id
  const storageKey = `${MEETING_PLAYER_STORAGE_KEY}.${personId}`
  const [ placement, setPlacement ] = useState(() => readPlayerPlacement(storageKey))

  useEffect(() => {
    function followOtherTab(event: StorageEvent) {
      if (event.key === storageKey) {
        setPlacement(readPlayerPlacement(storageKey))
      }
    }

    window.addEventListener('storage', followOtherTab)
    return () => window.removeEventListener('storage', followOtherTab)
  }, [ storageKey ])

  const save = useCallback((next: PlayerPlacement) => {
    setPlacement(next)
    try {
      window.localStorage.setItem(storageKey, JSON.stringify(next))
    }
    catch (error) {
      console.debug('The meeting player\'s place could not be saved to localStorage', { error })
    }
  }, [ storageKey ])

  return { placement, move: setPlacement, save } as const
}
