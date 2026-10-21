// Copyright © 2026 Jalapeno Labs

import type { TimelineRecord } from '../../api/routes/timelineRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import {
  attributionFor,
  confidencePercent,
  isElysiaAttribution,
  isVideoClip,
  segmentDetails,
  sourceLabelKey,
} from './timelinePresentation'

function record(overrides: Partial<TimelineRecord> = {}): TimelineRecord {
  return {
    id: 'record-1',
    sourceKind: 'meeting',
    connectorAccountId: 'account-1',
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
    expect(sourceLabelKey('slack')).toBe('sources.other')
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
  it('finds the attribution to one project and tells who made it', () => {
    const attribution = attributionFor(record(), 'project-1')
    expect(attribution?.confidence).toBe(0.84)
    expect(attribution && isElysiaAttribution(attribution)).toBe(true)
    expect(attributionFor(record(), 'project-2')).toBeNull()
  })

  it('shows a confidence as a bounded whole percentage', () => {
    expect(confidencePercent(0.84)).toBe(84)
    expect(confidencePercent(1.4)).toBe(100)
    expect(confidencePercent(-0.2)).toBe(0)
  })
})
