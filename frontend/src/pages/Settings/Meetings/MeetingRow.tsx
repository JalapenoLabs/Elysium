// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../../api/routes/meetingRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link as RouterLink } from 'react-router'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { meetingUpserted } from '../../../store/meetingsSlice'

// User interface
import { Button, Chip, Link, toast } from '@heroui/react'
import { ConnectorLogo } from '../Connectors/ConnectorLogo'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { retryMeeting } from '../../../api/routes/meetingRoutes'
import { formatDateTime } from '../../../format/dates'
import { formatDuration } from '../../../format/durations'
import { getConnectorUrl } from '../../../urls'
import { meetingStateChipColors, meetingStateLabelKeys } from './meetingsPresentation'

type Props = {
  meeting: Meeting
}

// One meeting: its title, the source it came from, when it was and how long, how far its
// processing got, and why it failed with the button to try again.
export function MeetingRow(props: Props) {
  const { t } = useTranslation([ 'meetings', 'connectors', 'common' ])
  const dispatch = useAppDispatch()
  const [ isRetrying, setIsRetrying ] = useState(false)
  const meeting = props.meeting
  // A source may not say how long the meeting ran.
  const duration = meeting.durationMs === null
    ? null
    : formatDuration(meeting.durationMs)
  const sourceName = t(`connectors:kinds.${meeting.sourceKind}.name`)

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
    formatDateTime(meeting.startedAt),
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
      <p className='flex flex-wrap items-center gap-x-1.5 text-xs'>
        <ConnectorLogo kind={meeting.sourceKind} className='size-3.5' />
        {meeting.connectorId
          ? <RouterLink to={getConnectorUrl(meeting.connectorId)} className='opacity-80 hover:underline'>{
            sourceName
          }</RouterLink>
          : <span className='opacity-80'>{sourceName}</span>}
        <span aria-hidden className='opacity-60'>·</span>
        <span className='opacity-60'>{details.filter(Boolean).join(' · ')}</span>
      </p>
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
