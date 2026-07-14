// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useRef } from 'react'

// Redux
import { dispatch } from '../store'
import { useAppSelector } from '../store/hooks'
import { currentUserLoaded, currentUserPresenceChanged, toCurrentUser } from '../store/currentUserSlice'
import { orgPatched } from '../store/sessionContextSlice'

// Misc
import { subscribeRealtime, getStreamHealth } from './client'
import { resolveSession } from '../auth/session'

// A tab hidden longer than this refetches `/context` on refocus even if the
// stream stayed healthy, as a cheap safety net against silent divergence.
const REFOCUS_REFETCH_AFTER_MS = 60_000

// Wires the session-level realtime kinds into the store, once, from the app
// chrome, plus the refocus reconciliation. Feature surfaces read the seeded
// slices; only cross-cutting concerns live here:
//   profile.updated  -> replace the cached profile when it's ours
//   presence.changed -> live presence flips for our own chip
//   org.updated      -> patch the org in the session context (name/logo)
//   session.changed  -> the account set changed somewhere; re-resolve
export function useRealtimeSession() {
  const subject = useAppSelector((state) => state.session.principal?.subject)
  const currentUserId = useAppSelector((state) => state.currentUser.user?.id)

  useEffect(() => {
    if (!subject) {
      return undefined
    }

    const unsubscribers = [
      subscribeRealtime('profile.updated', (envelope) => {
        if (envelope.payload.case !== 'profileUpdated') {
          return
        }
        const user = envelope.payload.value.user
        if (user?.subject === subject) {
          dispatch(currentUserLoaded(toCurrentUser(user)))
        }
      }),

      subscribeRealtime('presence.changed', (envelope) => {
        if (envelope.payload.case !== 'presenceChanged') {
          return
        }
        const changed = envelope.payload.value
        if (currentUserId && changed.userId?.value === currentUserId) {
          dispatch(currentUserPresenceChanged(changed.presence))
        }
      }),

      subscribeRealtime('org.updated', (envelope) => {
        if (envelope.payload.case !== 'orgUpdated') {
          return
        }
        const updated = envelope.payload.value
        dispatch(orgPatched({
          id: updated.id?.value ?? '',
          name: updated.name,
          logoUrl: updated.logoUrl,
          accentColor: updated.accentColor,
          wordmark: updated.wordmark,
        }))
      }),

      subscribeRealtime('session.changed', (envelope) => {
        if (envelope.payload.case !== 'sessionChanged') {
          return
        }
        console.debug('session changed server-side, re-resolving', {
          reason: envelope.payload.value.reason,
        })
        void resolveSession()
      }),
    ]

    return () => {
      for (const unsubscribe of unsubscribers) {
        unsubscribe()
      }
    }
  }, [ subject, currentUserId ])

  // Refocus reconciliation: refetch `/context` in the background only when the
  // stream dropped while the tab was hidden, or the tab slept past the
  // threshold. A healthy stream over a short blur costs nothing.
  const hiddenAtRef = useRef<number | null>(null)
  useEffect(() => {
    function onVisibilityChange() {
      if (document.visibilityState === 'hidden') {
        hiddenAtRef.current = Date.now()
        return
      }

      const hiddenAt = hiddenAtRef.current
      hiddenAtRef.current = null
      if (hiddenAt === null) {
        return
      }

      const health = getStreamHealth()
      const droppedWhileHidden = health.lastDisconnectedAt !== null
        && health.lastDisconnectedAt >= hiddenAt
      const hiddenTooLong = Date.now() - hiddenAt > REFOCUS_REFETCH_AFTER_MS

      if (!health.isConnected || droppedWhileHidden || hiddenTooLong) {
        void resolveSession()
      }
    }

    document.addEventListener('visibilitychange', onVisibilityChange)
    return () => document.removeEventListener('visibilitychange', onVisibilityChange)
  }, [])
}
