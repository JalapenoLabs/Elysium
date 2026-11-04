// Copyright © 2026 Jalapeno Labs

import type { TimelineRecord } from '../api/routes/timelineRoutes'

// Misc
import { useConnectorKinds } from '../pages/Settings/Connectors/useConnectorKinds'

// How `record` is answered, as its kind declares, or null when nothing answers it: its kind
// replies never, or it came from no connector.
export function useRecordReplies(record: TimelineRecord | null) {
  const connectorKinds = useConnectorKinds()
  const replies = connectorKinds.kinds
    .find((definition) => definition.kind === record?.sourceKind)
    ?.replies
  if (!record?.connectorId || !replies || replies.through === 'never') {
    return null
  }
  return replies
}
