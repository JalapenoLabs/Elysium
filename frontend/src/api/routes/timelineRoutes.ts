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
  connectorAccountId: string | null
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
}

export type TimelineQuery = {
  projectId?: string
  // An `occurredAt`: only records before it, for the next page.
  before?: string
  limit?: number
}

type ListTimelineResponse = {
  records: TimelineRecord[]
}

// The SWR key a page of the timeline loads under. The event stream revalidates every key
// that starts with `v1/timeline` when something new is ingested.
export function timelineKey(query: TimelineQuery) {
  const parameters = new URLSearchParams()
  if (query.projectId) {
    parameters.set('projectId', query.projectId)
  }
  if (query.before) {
    parameters.set('before', query.before)
  }
  if (query.limit) {
    parameters.set('limit', String(query.limit))
  }
  const search = parameters.toString()
  return search
    ? `v1/timeline?${search}`
    : 'v1/timeline'
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

// The caller says the record is about the project, with certainty.
export function attributeTimelineRecord(recordId: string, projectId: string) {
  return apiClient
    .put(`v1/timeline/${recordId}/projects/${projectId}`)
    .json<TimelineRecordResponse>()
}

export function unattributeTimelineRecord(recordId: string, projectId: string) {
  return apiClient
    .delete(`v1/timeline/${recordId}/projects/${projectId}`)
    .json<TimelineRecordResponse>()
}

// Where a `<video>` or `<audio>` element plays a record's clip from, on Elysium's own origin.
export function getTimelineMediaUrl(recordId: string) {
  return `${API_BASE_PATH}/v1/timeline/${recordId}/media`
}
