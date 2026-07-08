// Copyright © 2026 Jalapeno Labs

// Core
import { useCallback } from 'react'

// Redux
import { useAppSelector } from '../store/hooks'

// Misc
import { logout } from '../api/routes/authRoutes'
import { API_BASE_URL } from '../constants'

// Exposes the current session plus sign-in / sign-out actions.
//
// Sign-in is a full-page navigation to the gateway login initiator: the browser
// never talks to Keycloak directly, and returns with a session cookie already
// set. Sign-out clears the cookie server-side and drops local state.
export function useSession() {
  const status = useAppSelector((state) => state.session.status)
  const principal = useAppSelector((state) => state.session.principal)

  const signIn = useCallback(() => {
    // Full-page navigation to the gateway login initiator; the browser never
    // talks to Keycloak directly and returns with a session cookie already set.
    window.location.assign(`${API_BASE_URL}/auth/login`)
  }, [])

  // `logout` clears the local session itself (in its `finally`), so this only
  // needs to swallow a failed request; the state is dropped either way.
  const signOut = useCallback(async () => {
    try {
      await logout()
    }
    catch (error) {
      console.debug('signOut request failed; local session already cleared', { error })
    }
  }, [])

  return {
    status,
    principal,
    isAuthenticated: status === 'authenticated',
    signIn,
    signOut,
  } as const
}
