// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../../api/routes/meetingRoutes'
import type { LoadStatus } from '../../../hooks/useServerData'

// Core
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { MeetingRow } from './MeetingRow'

// Misc
import { getTabUrl, UrlTree } from '../../../urls'

type Props = {
  status: LoadStatus
  // Newest first.
  meetings: Meeting[]
  // Whether any meeting source is connected. Without one, an empty list points to the Sources
  // tab, where one is connected.
  hasSources: boolean
}

// The person's recent meetings, newest first, each with the source it came from and how far its
// processing got. The Meetings page loads them, since its tab counts them too.
export function MeetingList(props: Props) {
  const { t } = useTranslation([ 'meetings', 'common' ])

  if (props.status === 'loading') {
    return <div className='grid place-items-center py-10'>
      <Spinner />
    </div>
  }
  if (props.status === 'failed') {
    return <p className='text-sm text-danger'>{t('common:errors.unexpected')}</p>
  }
  if (!props.meetings.length) {
    return <EmptyNotice>{
      props.hasSources
        ? t('list.empty')
        : <>
          <span>{t('list.noSources')}</span>
          {' '}
          <Link to={getTabUrl(UrlTree.settingsMeetings, 'sources')} className='text-link'>{
            t('list.connectSource')
          }</Link>
        </>
    }</EmptyNotice>
  }

  return <ul className='divide-y divide-separator rounded-xl border border-separator'>{
    props.meetings.map((meeting) => <MeetingRow key={meeting.id} meeting={meeting} />)
  }</ul>
}
