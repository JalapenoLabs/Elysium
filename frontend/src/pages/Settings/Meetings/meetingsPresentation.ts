// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { Connector, ConnectorKind, ConnectorKindDefinition } from '../../../api/routes/connectorRoutes'
import type { MeetingState } from '../../../api/routes/meetingRoutes'

type ChipColor = 'accent' | 'success' | 'warning' | 'danger' | 'default'

export const meetingStateLabelKeys = {
  pending: 'states.pending',
  recorded: 'states.recorded',
  segmented: 'states.segmented',
  done: 'states.done',
  failed: 'states.failed',
} as const satisfies Record<MeetingState, ParseKeys<'meetings'>>

// Still working reads as accent, finished as success, given up as danger.
export const meetingStateChipColors = {
  pending: 'accent',
  recorded: 'accent',
  segmented: 'accent',
  done: 'success',
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
