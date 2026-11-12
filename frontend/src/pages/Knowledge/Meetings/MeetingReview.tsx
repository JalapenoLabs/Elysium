// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../../api/routes/meetingRoutes'
import type { PageCrumb } from '../../../layout/page/PageHeader'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Chip, Link, Spinner, Tabs } from '@heroui/react'
import { LuExternalLink } from 'react-icons/lu'
import { PageTabs } from '../../../components/PageTabs'
import { Page } from '../../../layout/page/Page'
import { MeetingSourceLogo } from '../../Settings/Meetings/MeetingSourceLogo'
import { DeletedMeetingNotice } from './DeletedMeetingNotice'
import { MeetingTopics } from './MeetingTopics'
import { MeetingWorkspace } from './MeetingWorkspace'
import { MergedIntoNotice } from './MergedIntoNotice'

// Misc
import { DateText } from '../../../format/DateText'
import { formatDuration } from '../../../format/durations'
import { useMeetingTopicsLoader } from '../../../hooks/useServerData'
import { useTabParam } from '../../../hooks/useTabParam'
import {
  meetingSourceNameKey,
  meetingStateChipColors,
  meetingStateLabelKeys,
} from '../../Settings/Meetings/meetingsPresentation'
import { useMeetingWords } from './useMeetingWords'

// The review first; then the topics the meeting was cut into, until excerpts take their place
// (#241), since a topic on no project or the wrong one is corrected there.
const TABS = [ 'review', 'topics' ] as const

type Props = {
  meeting: Meeting
  parents: PageCrumb[]
}

// A meeting that loaded: its title and source, when it was, for how long, and a link to it at its
// source, then its review and its topics as tabs, the open one in `?tab=`.
export function MeetingReview(props: Props) {
  const { t } = useTranslation([ 'meetings', 'connectors' ])
  const { meeting } = props
  const [ selectedTab, selectTab ] = useTabParam(TABS)
  const topics = useMeetingTopicsLoader(meeting.id)
  const words = useMeetingWords(meeting.id)

  let review = <div className='grid place-items-center py-10'><Spinner /></div>
  if (words.hasFailed) {
    review = <p className='text-sm text-danger'>{t('review.loadError')}</p>
  }
  else if (words.transcript) {
    review = <MeetingWorkspace
      meeting={meeting}
      transcript={words.transcript}
      applyChange={words.applyChange}
      reload={words.reload}
    />
  }

  return <Page
    title={meeting.title || t('list.untitled')}
    icon={<MeetingSourceLogo sourceKind={meeting.sourceKind} className='size-8' />}
    parents={props.parents}
    // While its run waits for the person, its processing waits too; the correction's status in
    // the options says so, rather than a state that reads as work under way.
    badges={!meeting.awaitingApproval && <Chip
      size='sm'
      variant='soft'
      color={meetingStateChipColors[meeting.state]}
    >{
        t(meetingStateLabelKeys[meeting.state])
      }</Chip>}
  >
    <div className='relaxed flex flex-wrap items-center gap-x-2 gap-y-1 text-sm'>
      <DateText value={meeting.startedAt} className='opacity-70' />
      {meeting.durationMs !== null && <>
        <span aria-hidden className='opacity-70'>·</span>
        <span className='opacity-70'>{formatDuration(meeting.durationMs)}</span>
      </>}
      {meeting.url && <>
        <span aria-hidden className='opacity-70'>·</span>
        <Link href={meeting.url} target='_blank' rel='noreferrer' className='text-link text-sm'>
          <span>{t('page.openAtSource', { source: t(meetingSourceNameKey(meeting.sourceKind)) })}</span>
          <LuExternalLink className='ml-1 inline size-3.5' aria-hidden />
        </Link>
      </>}
    </div>

    {meeting.mergedInto && <MergedIntoNotice mergedInto={meeting.mergedInto} />}
    {meeting.deletedAt && <DeletedMeetingNotice meeting={meeting} />}

    <PageTabs
      label={t('page.tabs.label')}
      selectedKey={selectedTab}
      onSelectionChange={selectTab}
      tabs={[
        { id: 'review', label: t('page.tabs.review'), count: null },
        { id: 'topics', label: t('page.tabs.topics'), count: topics.total },
      ]}
    >
      <Tabs.Panel id='review' className='mt-0 p-0'>{review}</Tabs.Panel>
      <Tabs.Panel id='topics'>
        <MeetingTopics meeting={meeting} status={topics.status} />
      </Tabs.Panel>
    </PageTabs>
  </Page>
}
