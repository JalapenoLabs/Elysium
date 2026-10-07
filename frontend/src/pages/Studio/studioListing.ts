// Copyright © 2026 Jalapeno Labs

import type { CodingSession } from '../../api/routes/codingSessionRoutes'
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

// The API already chose the image (`api/src/studio/thumbnail.rs`), so this only decides what
// stands in for one: a working placeholder while the first turn runs, else an empty frame.
export function getTileThumbnail(item: StudioItem, latestSession: CodingSession | undefined): TileThumbnail {
  if (item.thumbnailAssetId) {
    return { kind: 'image', assetId: item.thumbnailAssetId }
  }

  if (!latestSession) {
    return { kind: 'empty' }
  }

  // A thread the API has not polled yet is one just started, with its first turn queued.
  const thread = latestSession.thread
  if (!thread) {
    return { kind: 'working' }
  }
  if (CLOSED_THREAD_STATES.includes(thread.state)) {
    return { kind: 'empty' }
  }

  const isWorking = WORKING_THREAD_STATES.some((state) => state === thread.state)
  if (isWorking || thread.queueDepth > 0) {
    return { kind: 'working' }
  }
  return { kind: 'empty' }
}
