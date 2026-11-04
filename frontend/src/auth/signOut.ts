// Copyright © 2026 Jalapeno Labs

// Misc
import { signOutSession } from '../api/routes/authRoutes'
import { stopEventStream } from '../realtime/eventStream'
import { UrlTree } from '../urls'

// Ends the session and opens the sign-in page with a full page load, so nothing this person
// loaded stays in memory for whoever signs in next. The API ends it with Kratos and clears the
// cookie, so the session stops working everywhere at once.
export async function signOut() {
  stopEventStream()
  try {
    await signOutSession()
  }
  catch (error) {
    // Kratos is unreachable: the sign-in page then shows whether the session lives on.
    console.debug('signOut could not end the session', { error })
  }
  window.location.assign(UrlTree.login)
}
