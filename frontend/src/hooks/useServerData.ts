// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect } from 'react'
import useSWR from 'swr'

// Redux
import { codingSessionsLoaded } from '../store/codingSessionsSlice'
import { useAppDispatch } from '../store/hooks'
import { githubCredentialsLoaded } from '../store/githubCredentialsSlice'
import { llmsLoaded } from '../store/llmsSlice'
import { mailAccountsLoaded } from '../store/mailAccountsSlice'
import { mailDomainsLoaded } from '../store/mailDomainsSlice'
import { mailServerUpdated } from '../store/mailServerSlice'
import { projectsLoaded } from '../store/projectsSlice'
import { satellitesLoaded } from '../store/satellitesSlice'
import { sessionHistoryLoaded, sessionTimelineOpened, sessionTimelineReleased } from '../store/sessionEventsSlice'
import { storageLocationsLoaded } from '../store/storageLocationsSlice'

// Misc
import { listCodingSessions, listSessionEvents } from '../api/routes/codingSessionRoutes'
import { listGithubCredentials } from '../api/routes/githubRoutes'
import { listLlms } from '../api/routes/llmRoutes'
import { getMailServer, listMailAccounts, listMailDomains } from '../api/routes/mailRoutes'
import { listProjects } from '../api/routes/projectRoutes'
import { listSatellites } from '../api/routes/satelliteRoutes'
import { listStorageLocations } from '../api/routes/storageRoutes'

// How server data reaches a component:
//
// 1. A component that shows server data calls one of these hooks. SWR fetches it once,
//    deduplicating every component that asks at the same time, and buffers the response.
// 2. The response lands in Redux, the source of truth components render from.
// 3. The event stream (`src/realtime/eventStream.ts`) keeps Redux current from then on,
//    and revalidates these keys whenever it reconnects.
//
// The hooks return only where the load stands. Components select the data from Redux.

type LoadStatus = 'loading' | 'loaded' | 'failed'

// `loaded` holds through revalidation and failed refetches: data already on screen
// stays there rather than flashing back to a spinner or an error.
function toLoadStatus(hasData: boolean, error: unknown): LoadStatus {
  if (hasData) {
    return 'loaded'
  }
  if (error) {
    return 'failed'
  }
  return 'loading'
}

// Collections dispatch from inside the fetcher, so only a response fresh off the network
// replaces Redux. SWR's buffered copy is older than the event stream's updates, and a
// component mounting on it must not roll Redux back.

export function useGithubCredentialsLoader(): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR('v1/github-credentials', async () => {
    const response = await listGithubCredentials()
    dispatch(githubCredentialsLoaded(response.credentials))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

export function useLlmsLoader(): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR('v1/llms', async () => {
    const response = await listLlms()
    dispatch(llmsLoaded(response.llms))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

export function useMailAccountsLoader(): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR('v1/mail/accounts', async () => {
    const response = await listMailAccounts()
    dispatch(mailAccountsLoaded(response.accounts))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

export function useMailServerLoader(): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR('v1/mail/server', async () => {
    const response = await getMailServer()
    dispatch(mailServerUpdated(response.server))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

export function useMailDomainsLoader(): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR('v1/mail/domains', async () => {
    const response = await listMailDomains()
    dispatch(mailDomainsLoaded(response.domains))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

export function useProjectsLoader(): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR('v1/projects', async () => {
    const response = await listProjects()
    dispatch(projectsLoaded(response.projects))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

export function useSatellitesLoader(): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR('v1/satellites', async () => {
    const response = await listSatellites()
    dispatch(satellitesLoaded(response.satellites))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

export function useStorageLocationsLoader(): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR('v1/storage-locations', async () => {
    const response = await listStorageLocations()
    dispatch(storageLocationsLoaded(response.locations))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

export function useCodingSessionsLoader(): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR('v1/coding-sessions', async () => {
    const response = await listCodingSessions()
    dispatch(codingSessionsLoaded(response.sessions))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

// Holds a session's timeline in Redux while the calling component is mounted. History
// merges by sequence rather than replacing, so SWR's buffered copy is safe to apply and
// a reopened conversation shows it at once while the fresh copy loads.
export function useSessionHistoryLoader(sessionId: string) {
  const dispatch = useAppDispatch()

  useEffect(() => {
    dispatch(sessionTimelineOpened(sessionId))
    return () => {
      dispatch(sessionTimelineReleased(sessionId))
    }
  }, [ dispatch, sessionId ])

  const { data, error, mutate } = useSWR(
    `v1/coding-sessions/${sessionId}/events`,
    () => listSessionEvents(sessionId),
  )

  useEffect(() => {
    if (data) {
      dispatch(sessionHistoryLoaded({ sessionId, history: data }))
    }
  }, [ dispatch, sessionId, data ])

  return {
    error,
    retry: () => mutate(),
  }
}
