// Copyright © 2026 Jalapeno Labs

import type { Layout } from 'react-resizable-panels'
import type { StudioLayout, StudioLayoutMode } from './studioLayout'

// Core
import { useEffect, useState, useSyncExternalStore } from 'react'

// Misc
import { STUDIO_LAYOUT_STORAGE_KEY, STUDIO_STACKED_LAYOUT_MAX_WIDTH_PX } from '../../constants'
import { getLayoutAfterResize, parseStoredStudioLayout } from './studioLayout'

// The layout is a per-browser convenience: storage may be unavailable, and anything it holds
// that this build cannot read falls back to the default.
function readStoredLayout() {
  try {
    return parseStoredStudioLayout(window.localStorage.getItem(STUDIO_LAYOUT_STORAGE_KEY))
  }
  catch (error) {
    console.debug('The Studio layout could not be read from localStorage', { error })
    return parseStoredStudioLayout(null)
  }
}

const STACKED_MEDIA_QUERY = `(width < ${STUDIO_STACKED_LAYOUT_MAX_WIDTH_PX}px)`

function subscribeToStackedQuery(onChange: () => void) {
  const query = window.matchMedia(STACKED_MEDIA_QUERY)
  query.addEventListener('change', onChange)
  return () => query.removeEventListener('change', onChange)
}

function readStackedQuery() {
  return window.matchMedia(STACKED_MEDIA_QUERY).matches
}

// The item page's layout: which columns show and their split, saved per browser, and
// whether the screen is narrow enough to stack them.
export function useStudioLayout() {
  const [ layout, setLayout ] = useState<StudioLayout>(readStoredLayout)
  const isStacked = useSyncExternalStore(subscribeToStackedQuery, readStackedQuery)

  useEffect(() => {
    try {
      window.localStorage.setItem(STUDIO_LAYOUT_STORAGE_KEY, JSON.stringify(layout))
    }
    catch (error) {
      console.debug('The Studio layout could not be saved to localStorage', { error })
    }
  }, [ layout ])

  return {
    layout,
    isStacked,
    changeMode: (mode: StudioLayoutMode) => setLayout((previous) => ({ ...previous, mode })),
    // The user dragged the split; a column dragged closed switches the mode.
    applyResize: (panels: Layout) => setLayout((previous) => getLayoutAfterResize(panels, previous)),
  } as const
}
