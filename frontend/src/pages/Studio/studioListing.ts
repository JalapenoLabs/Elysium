// Copyright © 2026 Jalapeno Labs

import type { CodingSession, ThreadState } from '../../api/routes/codingSessionRoutes'
import type { StudioItem } from '../../api/routes/studioRoutes'

// Misc
import { CLOSED_THREAD_STATES } from '../Coding/sessionPresentation'

// The grid's filters, kept in the address so a filtered grid survives opening an item and
// coming back.

// `project` filters on items in no project.
export const NO_PROJECT = 'none'

export type StudioFilters = {
  // A project id, `NO_PROJECT`, or null for any.
  project: string | null
  // Deleted items only, instead of live ones.
  deleted: boolean
}

const PROJECT_PARAM = 'project'
const DELETED_PARAM = 'deleted'

export function readStudioFilters(params: URLSearchParams): StudioFilters {
  return {
    project: params.get(PROJECT_PARAM),
    deleted: params.get(DELETED_PARAM) === 'true',
  }
}

// Only what differs from the defaults goes in the address, so the plain grid has a plain
// address.
export function writeStudioFilters(filters: StudioFilters) {
  const params = new URLSearchParams()
  if (filters.project) {
    params.set(PROJECT_PARAM, filters.project)
  }
  if (filters.deleted) {
    params.set(DELETED_PARAM, 'true')
  }
  return params
}

// The items in the chosen project, in the order given. `items` are already the live or the
// deleted ones, as `filters.deleted` chose; this does not re-check deletion.
export function filterStudioItems(items: StudioItem[], filters: StudioFilters) {
  if (!filters.project) {
    return items
  }
  const projectId = filters.project === NO_PROJECT
    ? null
    : filters.project
  return items.filter((item) => item.projectId === projectId)
}

// What a tile shows where its picture goes.
export type TileThumbnail =
  // The pinned image, or the default the API picked.
  | { kind: 'image', assetId: string }
  // No image yet, while the agent is at work on the item.
  | { kind: 'working' }
  // No image, and nothing running that would make one.
  | { kind: 'empty' }

// Thread states in which the agent is, or is about to be, at work.
const WORKING_THREAD_STATES = [ 'provisioning', 'running' ] as const

// Whether the agent is at work on an item, from its newest session: a turn runs or waits, or
// the thread was just started and the API has not polled it yet, with its first turn queued.
export function isSessionWorking(latestSession: CodingSession | undefined) {
  if (!latestSession) {
    return false
  }

  const thread = latestSession.thread
  if (!thread) {
    return true
  }
  if (CLOSED_THREAD_STATES.includes(thread.state)) {
    return false
  }
  const isRunning = WORKING_THREAD_STATES.some((state) => state === thread.state)
  return isRunning || thread.queueDepth > 0
}

// The API already chose the image (`api/src/studio/thumbnail.rs`), so this only decides what
// stands in for one: a working placeholder while the first turn runs, else an empty frame.
export function getTileThumbnail(item: StudioItem, latestSession: CodingSession | undefined): TileThumbnail {
  if (item.thumbnailAssetId) {
    return { kind: 'image', assetId: item.thumbnailAssetId }
  }
  if (isSessionWorking(latestSession)) {
    return { kind: 'working' }
  }
  return { kind: 'empty' }
}

// The thread state a tile's chip shows, or null for no chip. An item with no session has no
// thread to report, and a thread the API has not polled yet, or one whose satellite reported
// no known state, has no state worth naming: the thumbnail already says when work is under way.
export function getTileThreadState(latestSession: CodingSession | undefined): ThreadState | null {
  const state = latestSession?.thread?.state
  if (!state || state === 'unknown') {
    return null
  }
  return state
}
