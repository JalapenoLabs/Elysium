// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../../api/routes/meetingRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { meetingUpserted } from '../../../store/meetingsSlice'

// User interface
import { Button, Chip, Link, toast } from '@heroui/react'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { retryMeeting } from '../../../api/routes/meetingRoutes'
import { formatDuration, meetingStateChipColors, meetingStateLabelKeys } from './meetingsPresentation'

const startedFormatter = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' })

type Props = {
  meeting: Meeting
}

// One meeting: its title, when it was and how long, how far its processing got, and why it
// failed with the button to try again.
export function MeetingRow(props: Props) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const dispatch = useAppDispatch()
  const [ isRetrying, setIsRetrying ] = useState(false)
  const meeting = props.meeting
  const duration = formatDuration(meeting.durationMs)

  async function retry() {
    setIsRetrying(true)
    try {
      const response = await retryMeeting(meeting.id)
      dispatch(meetingUpserted(response.meeting))
      toast.success(t('list.retried'))
    }
    catch (error) {
      console.debug('MeetingRow failed to retry a meeting', { error, meetingId: meeting.id })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setIsRetrying(false)
    }
  }

  const details = [
    startedFormatter.format(new Date(meeting.startedAt)),
    duration,
    meeting.segmentCount === null
      ? null
      : t('list.segments', { count: meeting.segmentCount }),
    meeting.state === 'done' && !meeting.hasRecording
      ? t('list.noRecording')
      : null,
  ]

  return <li className='level items-start gap-4 px-4 py-3'>
    <div className='min-w-0'>
      <div className='flex flex-wrap items-center gap-2'>
        <span className='truncate font-medium'>{meeting.title || t('list.untitled')}</span>
        <Chip size='sm' variant='soft' color={meetingStateChipColors[meeting.state]}>{
          t(meetingStateLabelKeys[meeting.state])
        }</Chip>
      </div>
      <p className='text-xs opacity-60'>{details.filter(Boolean).join(' · ')}</p>
      {meeting.lastError && <p className='mt-1 text-sm text-danger'>{meeting.lastError}</p>}
    </div>
    <div className='flex shrink-0 items-center gap-2'>
      {meeting.url && <Link href={meeting.url} target='_blank' rel='noreferrer' className='text-sm'>{
        t('list.open')
      }</Link>}
      {meeting.state === 'failed' && <Button size='sm' variant='outline' isPending={isRetrying} onPress={retry}>
        <span>{t('list.retry')}</span>
      </Button>}
    </div>
  </li>
}
