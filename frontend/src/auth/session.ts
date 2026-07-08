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
} from '../store/sessionSlice'

// Misc
import { getMe } from '../api/routes/authRoutes'

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
      return
    }
    console.debug('resolveSession could not reach the identity service', { error })
    dispatch(sessionError())
  }
}
