// Copyright © 2026 Jalapeno Labs

import type { Session } from '@ory/client-fetch'

// Core
import useSWR from 'swr'

// Misc
import { kratos } from '../../../api/kratos'

// Where this person is signed in, straight from Kratos, this device first. Read by the
// Sessions tab and by its count, which share one request through SWR.
export function useSignedInSessions() {
  const { data: current } = useSWR('kratos/session', () => kratos.toSession())
  const { data: others, error, mutate } = useSWR('kratos/sessions', () => kratos.listMySessions())

  // Kratos lists every session but the one asking, so this device comes from whoami.
  const sessions: Session[] = []
  if (current) {
    sessions.push(current)
  }
  sessions.push(...(others ?? []))

  return {
    current,
    others,
    sessions,
    isLoaded: Boolean(others),
    loadFailed: Boolean(error),
    reload: mutate,
  } as const
}
