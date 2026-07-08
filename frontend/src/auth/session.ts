// Copyright © 2026 Jalapeno Labs

// Core
import { HTTPError } from 'ky'

// Redux
import { dispatch } from '../store'
import {
  sessionLoading,
  sessionAuthenticated,
  sessionAnonymous,
  sessionError,
  instanceClaimedResolved,
} from '../store/sessionSlice'

// Misc
import { getMe, getAuthContext } from '../api/routes/authRoutes'

// Resolves the browser's session against the API and records it in Redux.
//
// The session is an HttpOnly cookie the browser cannot read, so the only way to
// know whether we're signed in is to ask the API: a 200 from `/api/v1/me` means
// authenticated, a 401 means anonymous, and anything else means the identity
// service is unreachable (rendered as a degraded state rather than a silent
// anonymous view).
export async function resolveSession(): Promise<void> {
  dispatch(sessionLoading())

  try {
    const principal = await getMe()
    dispatch(sessionAuthenticated({
      subject: principal.subject,
      email: principal.email,
      username: principal.username,
    }))
  }
  catch (error) {
    if (error instanceof HTTPError && error.response.status === 401) {
      dispatch(sessionAnonymous())
      // Anonymous: also resolve whether this instance still needs its first-run
      // claim, so the sign-in gate can route to setup instead of asking an
      // unclaimed instance to sign in.
      await resolveInstanceClaimed()
      return
    }
    console.debug('resolveSession could not reach the identity service', { error })
    dispatch(sessionError())
  }
}

// Resolves whether the instance has been claimed by a root user, recording it in
// Redux. Best-effort: a failure leaves the flag unresolved rather than blocking
// sign-in.
async function resolveInstanceClaimed(): Promise<void> {
  try {
    const { claimed } = await getAuthContext()
    dispatch(instanceClaimedResolved(claimed))
  }
  catch (error) {
    console.debug('could not resolve the instance claim state', { error })
  }
}
