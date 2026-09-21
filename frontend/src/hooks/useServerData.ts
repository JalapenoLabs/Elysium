// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect } from 'react'
import useSWR from 'swr'

// Redux
import { actionItemCommentsLoaded } from '../store/actionItemCommentsSlice'
import { historyLoaded } from '../store/actionItemHistorySlice'
import { actionItemLinksLoaded } from '../store/actionItemLinksSlice'
import { initiativeLinksLoaded } from '../store/initiativeLinksSlice'
import { actionItemsLoaded, actionItemUpserted, deletedActionItemsLoaded } from '../store/actionItemsSlice'
import { changesetsLoaded, changesetUpserted } from '../store/changesetsSlice'
import { codingSessionsLoaded } from '../store/codingSessionsSlice'
import { useAppDispatch } from '../store/hooks'
import { environmentVariablesLoaded } from '../store/environmentVariablesSlice'
import { githubCredentialsLoaded } from '../store/githubCredentialsSlice'
import { deletedInitiativesLoaded, initiativesLoaded, initiativeUpserted } from '../store/initiativesSlice'
import { jiraCredentialsLoaded } from '../store/jiraCredentialsSlice'
import { llmsLoaded } from '../store/llmsSlice'
import { mailAccountsLoaded } from '../store/mailAccountsSlice'
import { mailDomainsLoaded } from '../store/mailDomainsSlice'
import { mailServerUpdated } from '../store/mailServerSlice'
import { projectsLoaded } from '../store/projectsSlice'
import { satellitesLoaded } from '../store/satellitesSlice'
import { sessionHistoryLoaded, sessionTimelineOpened, sessionTimelineReleased } from '../store/sessionEventsSlice'
import { storageLocationsLoaded } from '../store/storageLocationsSlice'

// Misc
import {
  getActionItem,
  listActionItemComments,
  listActionItemHistory,
  listActionItemLinks,
  listActionItems,
  readActionItemLinkRemotes,
} from '../api/routes/actionItemRoutes'
import {
  getInitiative,
  getInitiativeProgress,
  listInitiativeHistory,
  listInitiativeLinks,
  listInitiatives,
} from '../api/routes/initiativeRoutes'
import { getChangeset, listChangesets } from '../api/routes/changesetRoutes'
import { listCodingSessions, listSessionEvents } from '../api/routes/codingSessionRoutes'
import { listEnvironmentVariables } from '../api/routes/environmentRoutes'
import { listGithubCredentials } from '../api/routes/githubRoutes'
import { listJiraCredentials } from '../api/routes/jiraRoutes'
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

export type LoadStatus = 'loading' | 'loaded' | 'failed'

// `loaded` holds through revalidation and failed refetches: data already on screen
// stays there rather than flashing back to a spinner or an error.
export function toLoadStatus(hasData: boolean, error: unknown): LoadStatus {
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

export function useEnvironmentVariablesLoader(): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR('v1/environment-variables', async () => {
    const response = await listEnvironmentVariables()
    dispatch(environmentVariablesLoaded(response.variables))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

export function useGithubCredentialsLoader(): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR('v1/github-credentials', async () => {
    const response = await listGithubCredentials()
    dispatch(githubCredentialsLoaded(response.credentials))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

export function useJiraCredentialsLoader(): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR('v1/jira-credentials', async () => {
    const response = await listJiraCredentials()
    dispatch(jiraCredentialsLoaded(response.credentials))
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
export function useSessionHistoryLoader(sessionId: number) {
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

// Every live action item. Next, the inbox, the list, and the project and initiative pages
// all select from these, so one load serves the whole area.
export function useActionItemsLoader(): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR('v1/action-items', async () => {
    const response = await listActionItems()
    dispatch(actionItemsLoaded(response.items))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

// Deleted items, loaded only while a view shows them. `actionItem.deleted` carries only an
// id, so the event stream revalidates this key when one arrives.
export function useDeletedActionItemsLoader(isEnabled: boolean): LoadStatus {
  const dispatch = useAppDispatch()
  const key = isEnabled
    ? 'v1/action-items?deleted=true'
    : null
  const { data, error } = useSWR(key, async () => {
    const response = await listActionItems({ deleted: true })
    dispatch(deletedActionItemsLoaded(response.items))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

// One item, live or deleted, for its own page: an address can name a deleted item, which
// the collections above leave out until asked.
export function useActionItemLoader(itemId: string): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR(`v1/action-items/${itemId}`, async () => {
    const response = await getActionItem(itemId)
    dispatch(actionItemUpserted(response.item))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

// Replaces the item's comments, so it dispatches only a fresh response.
export function useActionItemCommentsLoader(itemId: string): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR(`v1/action-items/${itemId}/comments`, async () => {
    const response = await listActionItemComments(itemId)
    dispatch(actionItemCommentsLoaded({ actionItemId: itemId, comments: response.comments }))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

export function useActionItemHistoryLoader(itemId: string): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR(`v1/action-items/${itemId}/history`, async () => {
    const response = await listActionItemHistory(itemId)
    dispatch(historyLoaded(response.history))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

// Every changeset, newest first. The Action items area loads it for the pending count on its
// tab and on Next; `changeset.upserted` keeps it current.
export function useChangesetsLoader(): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR('v1/changesets', async () => {
    const response = await listChangesets()
    dispatch(changesetsLoaded(response.changesets))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

export function useChangesetLoader(changesetId: string): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR(`v1/changesets/${changesetId}`, async () => {
    const response = await getChangeset(changesetId)
    dispatch(changesetUpserted(response.changeset))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

export function useInitiativesLoader(): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR('v1/initiatives', async () => {
    const response = await listInitiatives()
    dispatch(initiativesLoaded(response.initiatives))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

// Deleted initiatives, loaded only while a view shows them, revalidated by the event stream
// on `initiative.deleted` like deleted items.
export function useDeletedInitiativesLoader(isEnabled: boolean): LoadStatus {
  const dispatch = useAppDispatch()
  const key = isEnabled
    ? 'v1/initiatives?deleted=true'
    : null
  const { data, error } = useSWR(key, async () => {
    const response = await listInitiatives({ deleted: true })
    dispatch(deletedInitiativesLoaded(response.initiatives))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

// One initiative, live or deleted, for its own page.
export function useInitiativeLoader(initiativeId: string): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR(`v1/initiatives/${initiativeId}`, async () => {
    const response = await getInitiative(initiativeId)
    dispatch(initiativeUpserted(response.initiative))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

export function useInitiativeHistoryLoader(initiativeId: string): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR(`v1/initiatives/${initiativeId}/history`, async () => {
    const response = await listInitiativeHistory(initiativeId)
    dispatch(historyLoaded(response.history))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

// The burnup is the one view read from SWR rather than Redux: no event carries it, and only
// the initiative's own page draws it. `initiative.upserted` carries the counts now, and the
// event stream revalidates this key with it, so the chart moves when progress does.
export function useInitiativeProgress(initiativeId: string) {
  const { data, error } = useSWR(
    `v1/initiatives/${initiativeId}/progress`,
    () => getInitiativeProgress(initiativeId),
  )
  return {
    progress: data,
    status: toLoadStatus(data !== undefined, error),
  } as const
}

export function useActionItemLinksLoader(itemId: string): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR(`v1/action-items/${itemId}/links`, async () => {
    const response = await listActionItemLinks(itemId)
    dispatch(actionItemLinksLoaded({ actionItemId: itemId, links: response.links }))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}

// What each linked thing looks like in its provider now, read live. Like the burnup, it is
// read from SWR rather than Redux: an item holds no provider field, and only the item's page
// shows them. `actionItemLink.upserted` revalidates the key, so a change the watcher read
// shows at once.
export function useActionItemLinkRemotes(itemId: string) {
  const { data, error } = useSWR(
    `v1/action-items/${itemId}/links/remote`,
    () => readActionItemLinkRemotes(itemId),
  )
  return {
    remotes: data?.remotes,
    status: toLoadStatus(data !== undefined, error),
  } as const
}

export function useInitiativeLinksLoader(initiativeId: string): LoadStatus {
  const dispatch = useAppDispatch()
  const { data, error } = useSWR(`v1/initiatives/${initiativeId}/links`, async () => {
    const response = await listInitiativeLinks(initiativeId)
    dispatch(initiativeLinksLoaded({ initiativeId, links: response.links }))
    return response
  })
  return toLoadStatus(data !== undefined, error)
}
