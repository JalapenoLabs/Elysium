// Copyright © 2026 Jalapeno Labs

import type { AttributionHistoryEntry, TimelineRecord } from '../../api/routes/timelineRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { CODING_AGENT_USER_ID, ELYSIA_USER_ID, PROJECT_MANAGER_USER_ID } from '../../constants'
import { makeProject } from '../../testFixtures'
import {
  boundedConfidence,
  describeAttributionChange,
  isElysiaAttribution,
  isVideoClip,
  projectsToAdd,
  isWorkspaceRecord,
  segmentDetails,
  sourceLabelKey,
  turnSummarySession,
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
    excerptMarkId: null,
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

// #193: a session's turn summaries are the workspace's records, and each opens its session at
// that turn.
describe('turnSummarySession', () => {
  function summary(metadata: Record<string, unknown>): TimelineRecord {
    return record({
      sourceKind: 'turn_summary',
      connectorId: null,
      externalId: 'turn:turn-7',
      actor: 'Session 12',
      title: 'Fixed the login form.',
      body: '',
      metadata,
      createdBy: CODING_AGENT_USER_ID,
    })
  }

  it('opens a Coding session on the Coding page at its turn', () => {
    expect(turnSummarySession(summary({ sessionId: 12, turnId: 'turn-7', studioItemId: null }))).toEqual({
      sessionId: 12,
      url: '/coding/12?turn=turn-7',
    })
  })

  it('opens a Studio session on its item\'s page at its turn', () => {
    const studio = summary({ sessionId: 12, turnId: 'turn-7', studioItemId: 'item-3' })
    expect(turnSummarySession(studio)?.url).toBe('/studio/item-3?turn=turn-7')
  })

  it('answers nothing for another kind of record, or a summary naming no turn', () => {
    expect(turnSummarySession(record())).toBeNull()
    expect(turnSummarySession(summary({ sessionId: 12 }))).toBeNull()
  })
})

describe('isWorkspaceRecord', () => {
  it('reads what Elysium\'s agents record as the workspace\'s, and a person\'s as theirs', () => {
    expect(isWorkspaceRecord(record({ createdBy: CODING_AGENT_USER_ID }))).toBe(true)
    expect(isWorkspaceRecord(record({ createdBy: PROJECT_MANAGER_USER_ID }))).toBe(true)
    expect(isWorkspaceRecord(record())).toBe(false)
  })
})

describe('sourceLabelKey for agents\' records', () => {
  it('names a turn summary', () => {
    expect(sourceLabelKey('turn_summary')).toBe('sources.turn_summary')
  })

  it('names a worker\'s merged pull request', () => {
    expect(sourceLabelKey('merged_pull_request')).toBe('sources.merged_pull_request')
  })
})
