// Copyright © 2026 Jalapeno Labs

// Misc
import { kratos } from '../api/kratos'
import { stopEventStream } from '../realtime/eventStream'
import { UrlTree } from '../urls'

// Ends the session with Kratos and opens the sign-in page with a full page load, so nothing
// this person loaded stays in memory for whoever signs in next.
export async function signOut() {
  stopEventStream()
  try {
    const flow = await kratos.createBrowserLogoutFlow()
    await kratos.updateLogoutFlow({ token: flow.logout_token })
  }
  catch (error) {
    // Signed out already, or Kratos is unreachable: either way, sign in again.
    console.debug('signOut could not end the session with Kratos', { error })
  }
  window.location.assign(UrlTree.login)
}
