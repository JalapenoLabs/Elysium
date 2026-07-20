// Copyright © 2026 Jalapeno Labs

// Opens the unified drive-search modal from anywhere (taskbar search, command
// palette, future surfaces). The modal binds on mount; an open requested
// before that is remembered and delivered on bind.
let openListener: ((query: string) => void) | null = null
let pendingQuery: string | null = null

export function openDriveSearch(query = '') {
  if (openListener) {
    openListener(query)
  }
  else {
    pendingQuery = query
  }
}

export function bindDriveSearchOpener(listener: (query: string) => void): () => void {
  openListener = listener
  if (pendingQuery !== null) {
    listener(pendingQuery)
    pendingQuery = null
  }

  return () => {
    if (openListener === listener) {
      openListener = null
    }
  }
}
