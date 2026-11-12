// Copyright © 2026 Jalapeno Labs

// Misc
import { API_BASE_PATH } from '../../constants'
import { apiClient } from '../index'

// Which project a record is about, and how sure whoever said so was.
export type Attribution = {
  projectId: string
  // 0 to 1; a person's own attribution is 1.
  confidence: number
  // `user:<id>` or `elysia`.
  attributedBy: string
  createdAt: string
}

// A meeting segment's span of its recording, and the clip cut from it when there is one.
export type MeetingSegmentMedia = {
  meetingId: string
  startMs: number
  endMs: number
  clip: {
    storageLocationId: string
    path: string
    contentType: string
  } | null
}

// One thing ingested onto the person's timeline: a mail, a meeting segment, and so on. Only
// its owner receives it.
export type TimelineRecord = {
  id: string
  // `outlook`, `gmail`, `meeting`, and so on.
  sourceKind: string
  connectorId: string | null
  externalId: string | null
  occurredAt: string
  // Who it came from, as the provider shows them.
  actor: string
  title: string
  body: string
  url: string | null
  media: MeetingSegmentMedia | null
  // Whatever else its source keeps, such as a segment's `summary`.
  metadata: Record<string, unknown>
  // The inbox item it raised, when it raised one.
  actionItemId: string | null
  projects: Attribution[]
  createdBy: string
  createdAt: string
  // The meeting excerpt it is, which its meeting's page opens at; null for any other record.
  excerptMarkId: string | null
}

export type TimelineQuery = {
  projectId?: string
  // Only the record that raised this action item.
  actionItemId?: string
  // Only this meeting's standing excerpts.
  meetingId?: string
  // An `occurredAt`: only records before it, for the next page.
  before?: string
  limit?: number
}

type ListTimelineResponse = {
  records: TimelineRecord[]
  // Every record the query matches across all pages, `before` and `limit` aside.
  total: number
}

// What every SWR key of the timeline starts with, which the event stream revalidates when
// something new is ingested, and a meeting's page when an excerpt changes.
export const timelineKeyPrefix = 'v1/timeline'

// The SWR key a page of the timeline loads under.
export function timelineKey(query: TimelineQuery) {
  const parameters = new URLSearchParams()
  if (query.projectId) {
    parameters.set('projectId', query.projectId)
  }
  if (query.actionItemId) {
    parameters.set('actionItemId', query.actionItemId)
  }
  if (query.meetingId) {
    parameters.set('meetingId', query.meetingId)
  }
  if (query.before) {
    parameters.set('before', query.before)
  }
  if (query.limit) {
    parameters.set('limit', String(query.limit))
  }
  const search = parameters.toString()
  return search
    ? `${timelineKeyPrefix}?${search}`
    : timelineKeyPrefix
}

export function listTimeline(query: TimelineQuery) {
  return apiClient
    .get(timelineKey(query))
    .json<ListTimelineResponse>()
}

type TimelineRecordResponse = {
  record: TimelineRecord
}

export function getTimelineRecord(recordId: string) {
  return apiClient
    .get(`v1/timeline/${recordId}`)
    .json<TimelineRecordResponse>()
}

// A change to what a record is attributed to: the record as it reads now, and the history entry
// the change recorded, which Undo reverses; null when nothing changed.
export type AttributionChangeResponse = {
  record: TimelineRecord
  entryId: string | null
}

// The caller says the record is about the project, with certainty.
export function attributeTimelineRecord(recordId: string, projectId: string) {
  return apiClient
    .put(`v1/timeline/${recordId}/projects/${projectId}`)
    .json<AttributionChangeResponse>()
}

export function unattributeTimelineRecord(recordId: string, projectId: string) {
  return apiClient
    .delete(`v1/timeline/${recordId}/projects/${projectId}`)
    .json<AttributionChangeResponse>()
}

// What an attribution held before or after a change.
export type HeldAttribution = {
  confidence: number
  // `user:<id>` or `elysia`.
  attributedBy: string
}

// Mirrors `HistoryEntryResponse` in api/src/routes/v1/timeline/mod.rs: one change to what a
// record is attributed to, Elysia's or a person's.
export type AttributionHistoryEntry = {
  id: string
  timelineRecordId: string
  projectId: string
  kind: 'attributed' | 'unattributed'
  // A person, or Elysia's machine user.
  createdBy: string
  // Null where there was no attribution.
  before: HeldAttribution | null
  after: HeldAttribution | null
  // The entry this one undid, when it is an undo.
  undid: string | null
  createdAt: string
}

type RecordHistoryResponse = {
  history: AttributionHistoryEntry[]
}

// The SWR key a record's history loads under. It starts with `v1/timeline`, so a change the
// event stream announces revalidates it with the rest of the timeline.
export function recordHistoryKey(recordId: string) {
  return `v1/timeline/${recordId}/history`
}

// Oldest first.
export function listRecordHistory(recordId: string) {
  return apiClient
    .get(recordHistoryKey(recordId))
    .json<RecordHistoryResponse>()
}

// Reverses one change; `409` (`attribution_changed`) when the attribution changed since.
export function undoRecordChange(recordId: string, entryId: string) {
  return apiClient
    .post(`v1/timeline/${recordId}/history/${entryId}/undo`)
    .json<AttributionChangeResponse>()
}

// Where a `<video>` or `<audio>` element plays a record's clip from, on Elysium's own origin.
export function getTimelineMediaUrl(recordId: string) {
  return `${API_BASE_PATH}/v1/timeline/${recordId}/media`
}
