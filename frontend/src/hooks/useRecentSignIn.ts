// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useState } from 'react'

// Misc
import { kratos } from '../api/kratos'
import { RECENT_SIGN_IN_MS, TWO_FACTOR_WORKING_TIME_MS } from '../constants'
import { getLoginUrl } from '../urls'

// Whether this person signed in recently enough to change how they sign in, with time left to
// finish. If not, it sends them to confirm it's them (`/login` in refresh mode), coming back to
// `returnTo`, before the page starts rather than when its last step is refused. Kratos and the
// API still check every change themselves; this only spares the person a half-done change.
export function useRecentSignIn(returnTo: string) {
  const [ isRecent, setIsRecent ] = useState(false)

  useEffect(() => {
    let isCurrent = true
    kratos.toSession()
      .then((session) => {
        if (!isCurrent) {
          return
        }
        const signedInAt = session.authenticated_at?.getTime() ?? 0
        if (Date.now() - signedInAt > RECENT_SIGN_IN_MS - TWO_FACTOR_WORKING_TIME_MS) {
          window.location.assign(getLoginUrl({ returnTo, refresh: true }))
          return
        }
        setIsRecent(true)
      })
      .catch((error: unknown) => {
        if (!isCurrent) {
          return
        }
        // The page's own requests report a session that is gone; this only looked ahead.
        console.debug('useRecentSignIn could not read the session, so the page starts as it is', { error })
        setIsRecent(true)
      })
    return () => {
      isCurrent = false
    }
  }, [ returnTo ])

  return isRecent
}
