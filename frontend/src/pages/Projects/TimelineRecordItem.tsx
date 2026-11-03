// Copyright © 2026 Jalapeno Labs

import type { TimelineRecord } from '../../api/routes/timelineRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { timelineRecordUpserted } from '../../store/timelineRecordsSlice'

// User interface
import { Button, Chip, Link, toast } from '@heroui/react'
import { SegmentClip } from './SegmentClip'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { attributeTimelineRecord, unattributeTimelineRecord } from '../../api/routes/timelineRoutes'
import { formatDuration } from '../Settings/Meetings/meetingsPresentation'
import {
  attributionFor,
  confidencePercent,
  isElysiaAttribution,
  segmentDetails,
  sourceLabelKey,
} from './timelinePresentation'

const occurredFormatter = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' })

type Props = {
  record: TimelineRecord
  projectId: string
}

// One record on a project's timeline. A meeting segment shows its summary, its clip, and its
// transcript excerpt on request; other records show their text. Elysia's attribution says how
// sure she was, and the person can confirm it or take the record off this project.
export function TimelineRecordItem(props: Props) {
  const { t } = useTranslation([ 'timeline', 'common' ])
  const dispatch = useAppDispatch()
  const [ isBusy, setIsBusy ] = useState(false)
  const [ showsTranscript, setShowsTranscript ] = useState(false)
  const record = props.record
  const attribution = attributionFor(record, props.projectId)
  const details = segmentDetails(record)
  const media = record.media

  async function decide(keep: boolean) {
    setIsBusy(true)
    try {
      const response = keep
        ? await attributeTimelineRecord(record.id, props.projectId)
        : await unattributeTimelineRecord(record.id, props.projectId)
      dispatch(timelineRecordUpserted(response.record))
      toast.success(keep
        ? t('attribution.confirmed')
        : t('attribution.removed'))
    }
    catch (error) {
      console.debug('TimelineRecordItem failed to change an attribution', { error, recordId: record.id })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setIsBusy(false)
    }
  }

  return <li className='rounded-xl border border-separator bg-surface p-4'>
    <div className='level items-start gap-3'>
      <div className='min-w-0'>
        <div className='flex flex-wrap items-center gap-2'>
          <Chip size='sm' variant='soft'>{t(sourceLabelKey(record.sourceKind))}</Chip>
          <span className='text-xs opacity-60'>{occurredFormatter.format(new Date(record.occurredAt))}</span>
          {record.actor && <span className='truncate text-xs opacity-60'>{record.actor}</span>}
        </div>
        <h3 className='mt-1 font-medium'>{record.title}</h3>
        {media && details.meetingTitle && <p className='text-xs opacity-60'>{
          t('segment.inMeeting', {
            meeting: details.meetingTitle,
            start: formatDuration(media.startMs),
            end: formatDuration(media.endMs),
          })
        }</p>}
      </div>
      {record.url && <Link href={record.url} target='_blank' rel='noreferrer' className='shrink-0 text-sm'>{
        t('openSource')
      }</Link>}
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

    {attribution && <div className='mt-3 flex flex-wrap items-center gap-2 text-xs'>
      <span className='opacity-60'>{
        isElysiaAttribution(attribution)
          ? t('attribution.byElysia', { percent: confidencePercent(attribution.confidence) })
          : t('attribution.byPerson')
      }</span>
      {isElysiaAttribution(attribution) && <Button
        size='sm'
        variant='ghost'
        isPending={isBusy}
        onPress={() => decide(true)}
      >
        <span>{t('attribution.confirm')}</span>
      </Button>}
      <Button size='sm' variant='ghost' isPending={isBusy} onPress={() => decide(false)}>
        <span>{t('attribution.remove')}</span>
      </Button>
    </div>}
  </li>
}
