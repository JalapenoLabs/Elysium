// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useState } from 'react'

// Whether the app may proceed to the session/login flow yet:
//   checking         - resolving the active connection (desktop only)
//   needs-connection - desktop shell with no instance selected; show the picker
//   ready            - a plain browser (same-origin), or a desktop instance is
//                      selected; the app can resolve its session
export type ConnectionGateStatus = 'checking' | 'needs-connection' | 'ready'

// Decides whether the desktop shell needs the user to pick an instance before
// anything else. In a plain browser `window.elysium` is undefined and the API is
// same-origin, so the gate is a no-op (`ready`). In the desktop shell the app
// must know which instance to talk to first. Selecting one reloads the window
// (the main process clears cookies and points API traffic at it), so this only
// reads state; it never has to react to a mid-session change. See docs/desktop.md.
export function useConnectionGate() {
  const bridge = typeof window === 'undefined' ? undefined : window.elysium?.connection

  const [ status, setStatus ] = useState<ConnectionGateStatus>(bridge ? 'checking' : 'ready')

  useEffect(() => {
    if (!bridge) {
      return undefined
    }
    let cancelled = false
    bridge.active()
      .then((server) => {
        if (!cancelled) {
          setStatus(server ? 'ready' : 'needs-connection')
        }
      })
      .catch((error) => {
        // If the active connection can't be read, the safest state is to ask.
        console.debug('could not read the active connection', { error })
        if (!cancelled) {
          setStatus('needs-connection')
        }
      })
    return () => {
      cancelled = true
    }
  }, [ bridge ])

  return status
}
