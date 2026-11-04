// Copyright © 2026 Jalapeno Labs

import type { TimelineRecord } from '../../api/routes/timelineRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Chip, Link } from '@heroui/react'
import { LuExternalLink } from 'react-icons/lu'
import { RecordProjects } from './RecordProjects'
import { SegmentClip } from './SegmentClip'
import { RecordReply } from '../../replies/RecordReply'

// Misc
import { formatDateTime } from '../../format/dates'
import { formatMediaPosition } from '../../format/durations'
import { segmentDetails, sourceLabelKey } from './timelinePresentation'

type Props = {
  record: TimelineRecord
}

// One record, on a project's timeline or a meeting's topics. A meeting segment shows its
// summary, its clip, and its transcript excerpt on request; other records show their text, and a
// message can be answered with Reply. Below, the projects it is on, which the person corrects.
export function TimelineRecordItem(props: Props) {
  const { t } = useTranslation([ 'timeline', 'common' ])
  const [ showsTranscript, setShowsTranscript ] = useState(false)
  const record = props.record
  const details = segmentDetails(record)
  const media = record.media

  return <li className='rounded-xl border border-separator bg-surface p-4'>
    <div className='level items-start gap-3'>
      <div className='min-w-0'>
        <div className='flex flex-wrap items-center gap-2'>
          <Chip size='sm' variant='soft'>{t(sourceLabelKey(record.sourceKind))}</Chip>
          <span className='text-xs opacity-60'>{formatDateTime(record.occurredAt)}</span>
          {record.actor && <span className='truncate text-xs opacity-60'>{record.actor}</span>}
        </div>
        <h3 className='mt-1 font-medium'>{record.title}</h3>
        {media && details.meetingTitle && <p className='text-xs opacity-60'>{
          t('segment.inMeeting', {
            meeting: details.meetingTitle,
            start: formatMediaPosition(media.startMs),
            end: formatMediaPosition(media.endMs),
          })
        }</p>}
      </div>
      {record.url && <Link
        href={record.url}
        target='_blank'
        rel='noreferrer'
        className='shrink-0 gap-1 text-sm'
      >
        <span>{t('openSource')}</span>
        <LuExternalLink className='size-3.5' aria-hidden />
      </Link>}
    </div>

    {details.summary && <p className='mt-2 text-sm'>{details.summary}</p>}

    {media && <div className='mt-3'>{
      media.clip
        ? <SegmentClip recordId={record.id} contentType={media.clip.contentType} />
        : <p className='text-xs opacity-60'>{t('segment.noClip')}</p>
    }</div>}

    {media && record.body && <div className='mt-2'>
      <Button size='sm' variant='ghost' onPress={() => setShowsTranscript(!showsTranscript)}>
        <span>{
          showsTranscript
            ? t('segment.hideTranscript')
            : t('segment.transcript')
        }</span>
      </Button>
      {showsTranscript && <pre
        className={'mt-2 max-h-80 overflow-auto rounded-lg bg-surface-secondary p-3 '
          + 'font-sans text-sm whitespace-pre-wrap'}
      >{
        record.body
      }</pre>}
    </div>}

    {!media && record.body && <p className='mt-2 line-clamp-3 text-sm opacity-80'>{record.body}</p>}

    <div className='mt-3 empty:hidden'>
      <RecordReply record={record} />
    </div>

    <div className='mt-3'>
      <RecordProjects record={record} />
    </div>
  </li>
}
