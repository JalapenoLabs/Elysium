// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { Attribution, TimelineRecord } from '../../api/routes/timelineRoutes'

// What each kind of record is called. A kind added later reads as a plain record until it
// gets its own name.
const sourceLabelKeys: Record<string, ParseKeys<'timeline'>> = {
  meeting: 'sources.meeting',
  outlook: 'sources.outlook',
  gmail: 'sources.gmail',
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

// Whether a clip plays as video; anything else plays as audio.
export function isVideoClip(contentType: string): boolean {
  return contentType.toLowerCase().startsWith('video/')
}

// The record's attribution to `projectId`, if it has one.
export function attributionFor(record: TimelineRecord, projectId: string): Attribution | null {
  return record.projects.find((attribution) => attribution.projectId === projectId) ?? null
}

// Whether Elysia, rather than a person, attributed it: hers can be confirmed or removed.
export function isElysiaAttribution(attribution: Attribution): boolean {
  return attribution.attributedBy === 'elysia'
}

// A confidence as a whole percentage, for "Elysia, 80% sure".
export function confidencePercent(confidence: number): number {
  return Math.round(Math.min(Math.max(confidence, 0), 1) * 100)
}
