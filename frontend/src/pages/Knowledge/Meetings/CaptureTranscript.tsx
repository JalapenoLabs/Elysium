// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr/immutable'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'

// Misc
import { getCaptureTranscript } from '../../../api/routes/meetingRoutes'
import { formatMediaPosition } from '../../../format/durations'

type Props = {
  meetingId: string
  captureId: string
}

// One capture's own transcript, read exactly as it came, for the options' switch between the final
// transcript and each capture (docs/meeting-review.md, The meeting's page). A capture is written
// once, by its fetch, and never again, so it is read once (`swr/immutable`). It carries no marks:
// they are the final transcript's.
export function CaptureTranscript(props: Props) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const { data, error } = useSWR(
    `v1/meetings/${props.meetingId}/captures/${props.captureId}/transcript`,
    () => getCaptureTranscript(props.meetingId, props.captureId),
  )

  if (!data) {
    if (error) {
      console.debug('CaptureTranscript could not load a capture', { error, captureId: props.captureId })
      return <p className='text-sm text-danger'>{t('common:errors.unexpected')}</p>
    }
    return <div className='grid place-items-center py-10'><Spinner /></div>
  }
  if (!data.transcript) {
    return <EmptyNotice>{t('review.captureEmpty')}</EmptyNotice>
  }

  return <div className='flex flex-col gap-4'>
    {data.transcript.basedOn === 'whisper' && <p className='text-sm opacity-70'>{t('review.captureFromWhisper')}</p>}
    {data.transcript.lines.map((line, index) => <div key={index}>
      <p className='mb-1 text-xs font-semibold'>
        <span>{line.speaker || t('review.noSpeaker')}</span>
        <span className='ml-2 font-normal opacity-60'>{formatMediaPosition(line.startMs)}</span>
      </p>
      <p className='leading-8'>{line.text}</p>
    </div>)}
  </div>
}
