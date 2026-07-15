// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { Navigate, useLocation } from 'react-router'

// Redux
import { useAppSelector } from '../store/hooks'

// UI
import { Splash } from '../pages/Auth/Splash'

// Misc
import { UrlTree } from '../urls'

type Props = {
  children: ReactNode
}

// Gates protected routes on the resolved session. While the session is still
// resolving it shows the splash; an unreachable identity service routes to a
// degraded state rather than silently showing an anonymous view. A member
// suspended in their active org is a valid session but has no access, so they
// are sent to the suspended screen instead of the app.
export function RequireAuth(props: Props) {
  const status = useAppSelector((state) => state.session.status)
  const activeOrgSuspended = useAppSelector((state) => state.sessionContext.activeOrgSuspended)
  const location = useLocation()

  if (status === 'authenticated') {
    if (activeOrgSuspended) {
      console.debug('RequireAuth: active account suspended in its org, routing to suspended')
      return <Navigate to={UrlTree.suspended} replace />
    }
    return props.children
  }
  if (status === 'error') {
    console.debug('RequireAuth: identity service unreachable, redirecting to identity-unavailable')
    return <Navigate to={UrlTree.identityUnavailable} replace />
  }
  if (status === 'anonymous') {
    console.debug('RequireAuth: no session, redirecting to login', { from: location.pathname })
    return <Navigate to={UrlTree.login} replace state={{ from: location.pathname }} />
  }

  // unknown | loading
  return <Splash />
}
