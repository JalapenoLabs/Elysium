// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { AttributionHistoryEntry, Attribution, TimelineRecord } from '../../api/routes/timelineRoutes'
import type { Project } from '../../api/routes/projectRoutes'

// Misc
import { ELYSIA_USER_ID, WORKSPACE_RECORD_OWNERS } from '../../constants'
import { getSessionTurnUrl } from '../../urls'

// What each kind of record is called. A kind added later reads as a plain record until it
// gets its own name.
const sourceLabelKeys: Record<string, ParseKeys<'timeline'>> = {
  meeting: 'sources.meeting',
  outlook: 'sources.outlook',
  gmail: 'sources.gmail',
  teams: 'sources.teams',
  slack: 'sources.slack',
  mail: 'sources.mail',
  project_manager: 'sources.project_manager',
  turn_summary: 'sources.turn_summary',
  merged_pull_request: 'sources.merged_pull_request',
}

export function sourceLabelKey(sourceKind: string): ParseKeys<'timeline'> {
  return sourceLabelKeys[sourceKind] ?? 'sources.other'
}

// A meeting segment's summary and meeting title, kept in its metadata, when it has them.
export function segmentDetails(record: TimelineRecord) {
  const summary = record.metadata.summary
  const meetingTitle = record.metadata.meetingTitle
  return {
    summary: typeof summary === 'string'
      ? summary
      : '',
    meetingTitle: typeof meetingTitle === 'string'
      ? meetingTitle
      : '',
  } as const
}

// Whether a record is the workspace's rather than one person's: what a Project Manager or a
// coding session put on its project's timeline. Nobody answers it or moves it between projects.
export function isWorkspaceRecord(record: TimelineRecord): boolean {
  return WORKSPACE_RECORD_OWNERS.includes(record.createdBy)
}

// The session a turn summary's record is of, and where its conversation opens at that turn, from
// what its metadata names (`crate::sessions::timeline`); `null` for any other record, or one
// whose metadata does not name them.
export function turnSummarySession(record: TimelineRecord) {
  if (record.sourceKind !== 'turn_summary') {
    return null
  }
  const { sessionId, turnId, studioItemId } = record.metadata
  if (typeof sessionId !== 'number' || typeof turnId !== 'string') {
    console.debug('turnSummarySession met a turn summary naming no session or turn', { recordId: record.id })
    return null
  }
  const itemId = typeof studioItemId === 'string'
    ? studioItemId
    : null
  return {
    sessionId,
    url: getSessionTurnUrl(sessionId, turnId, itemId),
  } as const
}

// Whether a clip plays as video; anything else plays as audio.
export function isVideoClip(contentType: string): boolean {
  return contentType.toLowerCase().startsWith('video/')
}

// Whether Elysia, rather than a person, attributed it: hers can be confirmed or removed.
export function isElysiaAttribution(attribution: Attribution): boolean {
  return attribution.attributedBy === 'elysia'
}

// How sure an attribution is, bounded, since a confidence outside 0 to 1 would read as nonsense.
export function boundedConfidence(confidence: number): number {
  return Math.min(Math.max(confidence, 0), 1)
}

// The projects a record can still be added to, every one it is not on yet, in `projects`' order.
export function projectsToAdd(record: TimelineRecord, projects: Project[]): Project[] {
  const attributed = new Set<string>()
  for (const attribution of record.projects) {
    attributed.add(attribution.projectId)
  }
  const addable: Project[] = []
  for (const project of projects) {
    if (!attributed.has(project.id)) {
      addable.push(project)
    }
  }
  return addable
}

// A change to what a record is about in words: its sentence, and how sure the attribution it
// left is, when it left one.
type AttributionChange = {
  key: ParseKeys<'timeline'>
  confidence: number | null
}

// One change to what a record is about, as its history says it. Only a record's owner changes
// it, besides Elysia, so a change that is not hers is the reader's own.
export function describeAttributionChange(entry: AttributionHistoryEntry): AttributionChange {
  const byElysia = entry.createdBy === ELYSIA_USER_ID
  const confidence = entry.after?.confidence ?? null
  if (entry.undid) {
    return { key: 'history.undid', confidence }
  }
  if (entry.kind === 'unattributed' && byElysia) {
    return { key: 'history.elysiaRemoved', confidence }
  }
  if (entry.kind === 'unattributed') {
    return { key: 'history.removed', confidence }
  }
  if (byElysia && entry.before) {
    return { key: 'history.elysiaResure', confidence }
  }
  if (byElysia) {
    return { key: 'history.elysiaAdded', confidence }
  }
  if (entry.before) {
    return { key: 'history.confirmed', confidence }
  }
  return { key: 'history.added', confidence }
}
