// Copyright © 2026 Jalapeno Labs

import type { MeetingCapture } from '../../../api/routes/meetingRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Tooltip } from '@heroui/react'
import { LuMoveHorizontal } from 'react-icons/lu'
import { MeetingSourceLogo } from '../../Settings/Meetings/MeetingSourceLogo'
import { CaptureOffsetEditor } from './CaptureOffsetEditor'

// Misc
import { formatDuration } from '../../../format/durations'
import { meetingSourceNameKey } from '../../Settings/Meetings/meetingsPresentation'
import { offsetSourceLabelKeys, offsetSourceOf, startOf } from './meetingCaptures'

type Props = {
  capture: MeetingCapture
  isEditing: boolean
  onEdit: () => void
  onCancel: () => void
  // Resolves whether the move applied.
  onMove: (offsetMs: number) => Promise<boolean>
}

// One capture in a meeting's options: where it came from, where it starts on the meeting's clock
// and how that was found, and Move, which opens its start in place to set it by hand.
export function CaptureRow(props: Props) {
  const { t } = useTranslation([ 'meetings', 'connectors' ])
  const { capture } = props
  const start = startOf(capture.offsetMs)
  const startText = t(start.key, { span: formatDuration(start.spanMs) })
  const found = offsetSourceOf(capture)
  const placed = found
    ? t('review.captures.placed', { start: startText, found: t(offsetSourceLabelKeys[found]) })
    : startText

  return <li className='flex flex-col gap-2'>
    <div className='flex items-start gap-2'>
      <MeetingSourceLogo sourceKind={capture.sourceKind} className='mt-0.5 size-5' />
      <div className='min-w-0 flex-1'>
        <p className='text-sm'>{t(meetingSourceNameKey(capture.sourceKind))}</p>
        <p className='text-xs opacity-70'>{placed}</p>
      </div>
      {!props.isEditing && <Tooltip delay={300}>
        <Button
          isIconOnly
          size='sm'
          variant='ghost'
          aria-label={t('review.captures.move')}
          onPress={props.onEdit}
        >
          <LuMoveHorizontal className='size-4' aria-hidden />
        </Button>
        <Tooltip.Content>{t('review.captures.move')}</Tooltip.Content>
      </Tooltip>}
    </div>
    {props.isEditing && <CaptureOffsetEditor
      captureId={capture.id}
      offsetMs={capture.offsetMs}
      onApply={props.onMove}
      onCancel={props.onCancel}
    />}
  </li>
}
