// Copyright © 2026 Jalapeno Labs

import type { PointerEvent as ReactPointerEvent, RefObject } from 'react'
import type { PlayerPlacement, Room } from './meetingPlayer'

// Core
import { useCallback, useEffect, useRef, useState } from 'react'

// Redux
import { selectMe } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// Utility
import { z } from 'zod'

// Misc
import { MEETING_PLAYER_STORAGE_KEY } from '../../../constants'
import { DEFAULT_PLAYER_PLACEMENT, placementAt } from './meetingPlayer'

const placementSchema = z.object({
  x: z.number().min(0).max(1),
  y: z.number().min(0).max(1),
})

type Drag = {
  pointerId: number
  // Where in the player the pointer took hold of it.
  offsetX: number
  offsetY: number
}

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

// Where one viewer left a meeting's floating player, the same for every meeting, and the drag that
// moves it there by its grip. A viewer is a person on a browser, as the page tree's memory is, so
// the key ends in their id. A drag moves the player as it goes and is saved once it ends, and a
// place saved in another tab is followed. `box` is the floating player and `room` the space it
// moves in, measured while it floats.
export function usePlayerPlacement(box: RefObject<HTMLDivElement | null>, room: Room | null) {
  const personId = useAppSelector(selectMe)?.user.id
  const storageKey = `${MEETING_PLAYER_STORAGE_KEY}.${personId}`
  const [ placement, setPlacement ] = useState(() => readPlayerPlacement(storageKey))
  const drag = useRef<Drag | null>(null)

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

  // Where the pointer has the player now, kept inside the window.
  function placeAt(event: ReactPointerEvent<HTMLElement>) {
    const current = drag.current
    if (!current || current.pointerId !== event.pointerId || !room) {
      return null
    }
    return placementAt(event.clientX - current.offsetX, event.clientY - current.offsetY, room)
  }

  function endDrag(event: ReactPointerEvent<HTMLElement>) {
    const next = placeAt(event)
    drag.current = null
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId)
    }
    if (next) {
      save(next)
    }
  }

  const gripHandlers = {
    onPointerDown: (event: ReactPointerEvent<HTMLElement>) => {
      const rect = box.current?.getBoundingClientRect()
      if (!rect || event.button !== 0) {
        return
      }
      drag.current = {
        pointerId: event.pointerId,
        offsetX: event.clientX - rect.left,
        offsetY: event.clientY - rect.top,
      }
      event.currentTarget.setPointerCapture(event.pointerId)
      event.preventDefault()
    },
    onPointerMove: (event: ReactPointerEvent<HTMLElement>) => {
      const next = placeAt(event)
      if (next) {
        setPlacement(next)
      }
    },
    onPointerUp: endDrag,
    onPointerCancel: endDrag,
  }

  return { placement, gripHandlers } as const
}
