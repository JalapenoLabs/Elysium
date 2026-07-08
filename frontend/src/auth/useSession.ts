// Copyright © 2026 Jalapeno Labs

// Core
import { useCallback } from 'react'

// Redux
import { useAppSelector } from '../store/hooks'

// Misc
import { logout } from '../api/routes/authRoutes'

// Exposes the current session plus a sign-out action.
//
// Signing in is a native passkey ceremony owned by the sign-in screen (see
// pages/Auth/SignIn): it needs the user's email and drives `navigator.credentials`,
// so it is not a context-free action to expose here. The browser never talks to any
// identity provider directly.
export function useSession() {
  const status = useAppSelector((state) => state.session.status)
  const principal = useAppSelector((state) => state.session.principal)

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
    signOut,
  } as const
}
