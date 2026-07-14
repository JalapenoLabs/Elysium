// Copyright © 2026 Jalapeno Labs

// Core
import { HTTPError } from 'ky'

// Redux
import { dispatch } from '../store'
import {
  sessionLoading,
  sessionAnonymous,
  sessionError,
  instanceClaimedResolved,
} from '../store/sessionSlice'

// Misc
import { getAuthContext } from '../api/routes/authRoutes'
import { getSessionContext } from '../api/routes/sessionRoutes'
import { startRealtime, stopRealtime, shareContextSnapshot } from '../realtime/client'
import { seedFromContext } from '../realtime/seedContext'

// Deduplicates concurrent session probes. React StrictMode invokes mount effects
// twice in dev, and route guards can trigger a resolve while one is already in
// flight, so callers share a single request instead of hitting the API twice. A
// fresh probe runs once the previous settles (e.g. a retry or a window refocus).
let inFlightResolve: Promise<void> | null = null

// Resolves the browser's session against the API and records it in Redux.
//
// The session is an HttpOnly cookie the browser cannot read, so the only way to
// know whether we're signed in is to ask the API. The bootstrap is ordered for
// realtime correctness (docs/realtime.md): the SSE stream connects FIRST, then
// `GET /api/v1/context` seeds every session slice in one response carrying the
// stream's high-water mark, so an event can neither slip between the fetch and
// the subscribe nor double-apply onto the state it is already part of.
export function resolveSession(): Promise<void> {
  if (inFlightResolve) {
    return inFlightResolve
  }
  inFlightResolve = runResolveSession().finally(() => {
    inFlightResolve = null
  })
  return inFlightResolve
}

async function runResolveSession(): Promise<void> {
  dispatch(sessionLoading())

  // Subscribe before reading; idempotent when the stream is already up.
  startRealtime()

  try {
    const { context, encoded } = await getSessionContext()
    seedFromContext(context)
    // The worker holds it for instant restore in future windows, and siblings
    // reseed from the rebroadcast.
    shareContextSnapshot({ data: encoded, asOfSequence: Number(context.asOfSequence) })
  }
  catch (error) {
    if (error instanceof HTTPError && error.response.status === 401) {
      // Anonymous: don't leave a stream hammering an endpoint that will 401.
      stopRealtime()
      dispatch(sessionAnonymous())
      // Also resolve whether this instance still needs its first-run claim, so
      // the login gate can route to setup instead of asking an unclaimed
      // instance to sign in.
      await resolveInstanceClaimed()
      return
    }
    console.debug('resolveSession could not reach the identity service', { error })
    stopRealtime()
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
