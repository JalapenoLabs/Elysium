// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { Connector, ConnectorKind, ConnectorKindDefinition } from '../../../api/routes/connectorRoutes'
import type { Meeting, MeetingState } from '../../../api/routes/meetingRoutes'

// Misc
import { CONNECTOR_KINDS } from '../../../api/routes/connectorRoutes'
import { UPLOAD_SOURCE_KIND } from '../../../api/routes/meetingRoutes'

type ChipColor = 'accent' | 'success' | 'warning' | 'danger' | 'default'

// Where a meeting's corrected transcript stands: not corrected yet, its run held for the
// person's approval, approved, or corrected and gone on without it.
export type CorrectionStatus = 'uncorrected' | 'awaitingApproval' | 'approved' | 'unapproved'

export function correctionStatusOf(meeting: Pick<Meeting, 'correction' | 'awaitingApproval'>): CorrectionStatus {
  if (!meeting.correction) {
    return 'uncorrected'
  }
  if (meeting.correction.approvedAt) {
    return 'approved'
  }
  if (meeting.awaitingApproval) {
    return 'awaitingApproval'
  }
  return 'unapproved'
}

export const correctionStatusLabelKeys = {
  uncorrected: 'correction.status.uncorrected',
  awaitingApproval: 'correction.status.awaitingApproval',
  approved: 'correction.status.approved',
  unapproved: 'correction.status.unapproved',
} as const satisfies Record<CorrectionStatus, ParseKeys<'meetings'>>

// Waiting on the person is the one that asks for attention.
export const correctionStatusChipColors = {
  uncorrected: 'default',
  awaitingApproval: 'warning',
  approved: 'success',
  unapproved: 'default',
} as const satisfies Record<CorrectionStatus, ChipColor>

export const meetingStateLabelKeys = {
  pending: 'states.pending',
  recorded: 'states.recorded',
  segmented: 'states.segmented',
  done: 'states.done',
  skipped: 'states.skipped',
  failed: 'states.failed',
} as const satisfies Record<MeetingState, ParseKeys<'meetings'>>

// Still working reads as accent, finished as success, skipped as nothing to see, and a failed
// run as danger.
export const meetingStateChipColors = {
  pending: 'accent',
  recorded: 'accent',
  segmented: 'accent',
  done: 'success',
  skipped: 'default',
  failed: 'danger',
} as const satisfies Record<MeetingState, ChipColor>

// Where meetings come from, without naming a source: every kind whose definition says it
// provides meetings, and the person's connectors of those kinds, each in its list's order.
export function meetingSourcesOf(definitions: ConnectorKindDefinition[], connectors: Connector[]) {
  const kinds: ConnectorKind[] = []
  for (const definition of definitions) {
    if (definition.provides.includes('meetings')) {
      kinds.push(definition.kind)
    }
  }

  const connected = connectors.filter((connector) => kinds.includes(connector.kind))
  return { kinds, connectors: connected } as const
}

// The connector kind a meeting's source is, when it is one. A source that is no connector, such
// as an upload, or a kind this build does not know yet, is none.
export function connectorKindOf(sourceKind: string) {
  return CONNECTOR_KINDS.find((kind) => kind === sourceKind) ?? null
}

// What a meeting's source is called: its connector kind's name, Uploaded for a person's upload, or
// a plain name for any other source that is no connector this build knows.
export function meetingSourceNameKey(sourceKind: string): ParseKeys<[ 'meetings', 'connectors' ]> {
  if (sourceKind === UPLOAD_SOURCE_KIND) {
    return 'meetings:upload.sourceName'
  }
  const kind = connectorKindOf(sourceKind)
  if (!kind) {
    console.debug('meetingSourceNameKey found a source that is no connector kind', { sourceKind })
    return 'meetings:sources.other'
  }
  return `connectors:kinds.${kind}.name`
}
