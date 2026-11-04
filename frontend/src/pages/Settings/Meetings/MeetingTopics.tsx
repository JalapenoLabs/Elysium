// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../../api/routes/meetingRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectTimelineRecordsByMeetingId } from '../../../store/timelineRecordsSlice'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { TimelineRecordItem } from '../../Projects/TimelineRecordItem'

type Props = {
  meeting: Meeting
  status: 'loading' | 'loaded' | 'failed'
}

// A meeting's topics in the order they were said, each with its clip, summary, and excerpt, and
// the projects Elysia put it on, which the person corrects here: a topic on no project yet
// reaches a project's timeline from this list. The page loads them, to count them on its tab.
export function MeetingTopics(props: Props) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const topics = useAppSelector((state) => selectTimelineRecordsByMeetingId(state, props.meeting.id))

  if (props.status === 'loading' && !topics.length) {
    return <div className='grid place-items-center py-10'><Spinner /></div>
  }
  if (props.status === 'failed' && !topics.length) {
    return <p className='text-sm text-danger'>{t('topics.loadError')}</p>
  }
  if (!topics.length) {
    return <EmptyNotice>{
      props.meeting.state === 'done'
        ? t('topics.none')
        : t('topics.empty')
    }</EmptyNotice>
  }
  return <ol className='flex flex-col gap-3'>{
    topics.map((topic) => <TimelineRecordItem
      key={topic.id}
      record={topic}
    />)
  }</ol>
}
