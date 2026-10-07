// Copyright © 2026 Jalapeno Labs

import type { CodingSession } from '../../api/routes/codingSessionRoutes'

// Misc
import { CLOSED_THREAD_STATES } from '../Coding/sessionPresentation'

// Where the item's next prompt goes, from its newest session:
// - `loading`: the item's sessions have not arrived, so it is not yet known.
// - `live`: the latest thread takes the prompt.
// - `ended`: the latest thread expired or was destroyed; the prompt continues the item in a new
//   thread, on a chosen satellite or else the latest session's.
// - `satelliteDeleted`: the latest session's satellite was deleted, so a satellite must be chosen.
// - `neverRan`: the item has no session at all (its creation failed), so a satellite must be
//   chosen to start it.
export type StudioThreadStatus = 'loading' | 'live' | 'ended' | 'satelliteDeleted' | 'neverRan'

// Mirrors the API (`api/src/studio/continuation.rs`): a continued item runs on the chosen
// satellite, else the latest session's, and answers 409 when there is neither.
export function getStudioThreadStatus(
  latestSession: CodingSession | undefined,
  areSessionsLoaded: boolean,
): StudioThreadStatus {
  if (!latestSession) {
    if (!areSessionsLoaded) {
      return 'loading'
    }
    return 'neverRan'
  }

  if (latestSession.satelliteId === null) {
    return 'satelliteDeleted'
  }

  // The thread is null until the API first polls it, and such a thread is live.
  const state = latestSession.thread?.state
  if (state && CLOSED_THREAD_STATES.includes(state)) {
    return 'ended'
  }

  return 'live'
}
