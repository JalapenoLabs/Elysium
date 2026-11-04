// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../../api/routes/meetingRoutes'

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Spinner } from '@heroui/react'
import { TextDiff } from '../../../components/TextDiff'

// Misc
import { getMeetingTranscript } from '../../../api/routes/meetingRoutes'
import { transcriptText } from './meetingsPresentation'

type Props = {
  meeting: Meeting
}

// A meeting's transcript as heard and as corrected, line for line: what correction changed as
// a diff, the unchanged stretches folded. Until it is corrected, or when nothing needed it, the
// transcript as heard. Its own read, since no other view shows a transcript; it is read again
// once the meeting's correction lands.
export function MeetingTranscript(props: Props) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const meeting = props.meeting
  const { data, error } = useSWR(
    [ 'v1/meetings/transcript', meeting.id, meeting.correction?.createdAt ?? null ],
    () => getMeetingTranscript(meeting.id),
    { keepPreviousData: true },
  )

  if (!data) {
    if (error) {
      console.debug('MeetingTranscript could not load a transcript', { error, meetingId: meeting.id })
      return <p className='text-sm text-danger'>{t('common:errors.unexpected')}</p>
    }
    return <div className='grid place-items-center py-10'><Spinner /></div>
  }

  const original = data.original
  const corrected = data.corrected
  const heard = original
    ? transcriptText(original.lines)
    : ''

  return <section>
    {!original && <p className='text-sm opacity-70'>{t('page.nothingHeard')}</p>}
    {original && corrected && Boolean(corrected.changedLines) && <TextDiff
      before={heard}
      after={transcriptText(corrected.lines)}
      label={t('page.diffLabel')}
      foldedLabel={(count) => t('page.foldedLines', { count })}
    />}
    {original && (!corrected || !corrected.changedLines) && <>
      {corrected && <p className='compact text-sm opacity-70'>{t('page.nothingChanged')}</p>}
      <div className='overflow-x-auto rounded-lg bg-surface-secondary px-3 py-2 font-mono text-xs whitespace-pre-wrap'>{
        heard
      }</div>
    </>}
  </section>
}
