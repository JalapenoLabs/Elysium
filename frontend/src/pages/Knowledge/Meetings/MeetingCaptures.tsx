// Copyright © 2026 Jalapeno Labs

import type { MeetingCapture } from '../../../api/routes/meetingRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { CaptureRow } from './CaptureRow'
import { MergedCaptureGroup } from './MergedCaptureGroup'

// Misc
import { usePendingAction } from '../../../hooks/usePendingAction'
import { groupCaptures } from './meetingCaptures'

// Resolves whether the capture moved.
type MoveCapture = (captureId: string, offsetMs: number) => Promise<boolean>

type Props = {
  captures: MeetingCapture[]
  // Null where the reader may not move a capture, which only the meeting's owner does.
  onMoveCapture: MoveCapture | null
  // Null where the reader may not split one out, which only its owner and admins do.
  onSplitCapture: ((captureId: string) => Promise<void>) | null
}

// A meeting's captures in its options (docs/meeting-review.md, The meeting's page): the meeting's
// own, then each meeting merged in, boxed with a link to it and Split out. Each capture moves on
// the meeting's clock by hand. Both act at once with Undo in their toast, as every change on the
// page does, and each is offered only to whoever may make it.
export function MeetingCaptures(props: Props) {
  const { t } = useTranslation('meetings')
  const [ editingId, setEditingId ] = useState<string | null>(null)
  const splitting = usePendingAction<string>()
  const groups = useMemo(() => groupCaptures(props.captures), [ props.captures ])
  const { onMoveCapture, onSplitCapture } = props

  async function move(onMove: MoveCapture, captureId: string, offsetMs: number) {
    const moved = await onMove(captureId, offsetMs)
    if (moved) {
      setEditingId(null)
    }
    return moved
  }

  return <section aria-label={t('review.captures.title')}>
    <p className='compact text-xs font-semibold uppercase opacity-60'>{t('review.captures.title')}</p>
    <div className='flex flex-col gap-3'>{
      groups.map((group) => {
        const rows = <ul className='flex flex-col gap-3'>{
          group.captures.map((capture) => <CaptureRow
            key={capture.id}
            capture={capture}
            isEditing={editingId === capture.id}
            onEdit={() => setEditingId(capture.id)}
            onCancel={() => setEditingId(null)}
            onMove={onMoveCapture && ((offsetMs) => move(onMoveCapture, capture.id, offsetMs))}
          />)
        }</ul>
        if (!group.from) {
          return <div key='own'>{rows}</div>
        }

        const fromId = group.from.meetingId
        // Any capture the merge brought names it; the API splits them all out together.
        const [ first ] = group.captures
        return <MergedCaptureGroup
          key={fromId}
          from={group.from}
          canSplit={group.canSplit}
          isSplitting={splitting.pendingKey === fromId}
          isDisabled={splitting.isOtherPending(fromId)}
          onSplit={onSplitCapture && (() => void splitting.run(fromId, () => onSplitCapture(first.id)))}
        >
          {rows}
        </MergedCaptureGroup>
      })
    }</div>
  </section>
}
