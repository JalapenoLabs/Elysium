// Copyright © 2026 Jalapeno Labs

import type { AttributionHistoryEntry, TimelineRecord } from '../../api/routes/timelineRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { ELYSIA_USER_ID } from '../../constants'
import { makeProject } from '../../testFixtures'
import {
  boundedConfidence,
  describeAttributionChange,
  isElysiaAttribution,
  isVideoClip,
  projectsToAdd,
  segmentDetails,
  sourceLabelKey,
} from './timelinePresentation'

function record(overrides: Partial<TimelineRecord> = {}): TimelineRecord {
  return {
    id: 'record-1',
    sourceKind: 'meeting',
    connectorId: 'connector-1',
    externalId: '01HX#0',
    occurredAt: '2026-10-21T15:01:00Z',
    actor: 'Ada',
    title: 'Launch',
    body: '[1:01] Ada: Launch is Friday.\n',
    url: null,
    media: null,
    metadata: { summary: 'Launch moves to Friday.', meetingTitle: 'Weekly sync' },
    actionItemId: null,
    projects: [
      { projectId: 'project-1', confidence: 0.84, attributedBy: 'elysia', createdAt: '2026-10-21T16:00:00Z' },
    ],
    createdBy: 'person-1',
    createdAt: '2026-10-21T16:00:00Z',
    ...overrides,
  }
}

describe('sourceLabelKey', () => {
  it('names known kinds and falls back for new ones', () => {
    expect(sourceLabelKey('meeting')).toBe('sources.meeting')
    expect(sourceLabelKey('outlook')).toBe('sources.outlook')
    expect(sourceLabelKey('slack')).toBe('sources.slack')
    expect(sourceLabelKey('a_kind_from_a_newer_build')).toBe('sources.other')
  })
})

describe('segmentDetails', () => {
  it('reads the summary and meeting title from metadata', () => {
    expect(segmentDetails(record())).toEqual({
      summary: 'Launch moves to Friday.',
      meetingTitle: 'Weekly sync',
    })
  })

  it('treats anything but text as absent', () => {
    expect(segmentDetails(record({ metadata: { summary: 3 }}))).toEqual({
      summary: '',
      meetingTitle: '',
    })
  })
})

describe('isVideoClip', () => {
  it('plays video types as video and the rest as audio', () => {
    expect(isVideoClip('video/mp4')).toBe(true)
    expect(isVideoClip('Video/WebM')).toBe(true)
    expect(isVideoClip('audio/mpeg')).toBe(false)
  })
})

describe('attributions', () => {
  it('tells who made an attribution and bounds how sure it reads', () => {
    const [ elysias ] = record().projects
    expect(isElysiaAttribution(elysias)).toBe(true)
    expect(isElysiaAttribution({ ...elysias, attributedBy: 'user:person-1' })).toBe(false)
    expect(boundedConfidence(0.84)).toBe(0.84)
    expect(boundedConfidence(1.4)).toBe(1)
    expect(boundedConfidence(-0.2)).toBe(0)
  })

  it('offers every project the record is not on yet, in their order', () => {
    const projects = [
      makeProject({ id: 'project-2', name: 'Budget' }),
      makeProject({ id: 'project-1', name: 'Launch' }),
      makeProject({ id: 'project-3', name: 'Website' }),
    ]
    expect(projectsToAdd(record(), projects).map((project) => project.id)).toEqual([ 'project-2', 'project-3' ])
  })
})

describe('describeAttributionChange', () => {
  function entry(overrides: Partial<AttributionHistoryEntry> = {}): AttributionHistoryEntry {
    return {
      id: 'entry-1',
      timelineRecordId: 'record-1',
      projectId: 'project-1',
      kind: 'attributed',
      createdBy: ELYSIA_USER_ID,
      before: null,
      after: { confidence: 0.72, attributedBy: 'elysia' },
      undid: null,
      createdAt: '2026-10-21T16:00:00Z',
      ...overrides,
    }
  }
  const persons = { confidence: 1, attributedBy: 'user:person-1' }

  it('says what Elysia did, and how sure she was', () => {
    expect(describeAttributionChange(entry())).toEqual({ key: 'history.elysiaAdded', confidence: 0.72 })
    expect(describeAttributionChange(entry({ before: { confidence: 0.5, attributedBy: 'elysia' }}))).toEqual({
      key: 'history.elysiaResure',
      confidence: 0.72,
    })
    expect(describeAttributionChange(entry({ kind: 'unattributed', before: entry().after, after: null }))).toEqual({
      key: 'history.elysiaRemoved',
      confidence: null,
    })
  })

  it('says what the person did, an undo above all', () => {
    const theirs = { createdBy: 'person-1' }
    expect(describeAttributionChange(entry({ ...theirs, after: persons })).key).toBe('history.added')
    expect(describeAttributionChange(entry({ ...theirs, before: entry().after, after: persons })).key)
      .toBe('history.confirmed')
    expect(describeAttributionChange(entry({ ...theirs, kind: 'unattributed', after: null })).key)
      .toBe('history.removed')
    expect(describeAttributionChange(entry({ ...theirs, undid: 'entry-0' })).key).toBe('history.undid')
  })
})
