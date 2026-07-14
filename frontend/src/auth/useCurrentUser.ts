// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect } from 'react'

// Redux
import { useAppSelector } from '../store/hooks'

// Misc
import { getMyProfile } from '../api/routes/userRoutes'

// Deduplicates the initial profile fetch across consumers: several chrome
// components render at once (profile badge, status control) and each should not
// fire its own `GET /users/me`. StrictMode's dev double-mount shares it too.
let inFlightLoad: Promise<void> | null = null

// The signed-in user's profile from the `currentUser` slice, fetching it once
// when authenticated and it hasn't loaded yet. Returns `null` until resolved
// (and always when anonymous; the slice self-clears on session end).
export function useCurrentUser() {
  const status = useAppSelector((state) => state.session.status)
  const currentUser = useAppSelector((state) => state.currentUser.user)

  useEffect(() => {
    if (status !== 'authenticated' || currentUser || inFlightLoad) {
      return
    }
    inFlightLoad = getMyProfile()
      .then(() => undefined)
      .catch((error) => {
        console.debug('could not load the current user profile', { error })
      })
      .finally(() => {
        inFlightLoad = null
      })
  }, [ status, currentUser ])

  return currentUser
}
