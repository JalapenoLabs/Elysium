// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../../api/routes/meetingRoutes'
import type { LoadStatus } from '../../../hooks/useServerData'

// Core
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { MeetingTable } from './MeetingTable'

// Misc
import { usePageToolsContainer } from '../../../layout/page/pageTools'
import { getTabUrl, UrlTree } from '../../../urls'

type Props = {
  status: LoadStatus
  // Newest first.
  meetings: Meeting[]
  // Whether any meeting source is connected. Without one, an empty list points to the Sources
  // tab, where one is connected.
  hasSources: boolean
}

// The Recent meetings tab: the person's meetings as the page's list, its search, Columns, and
// export in the titlebar. The Meetings page loads them, since its tab counts them too.
export function MeetingList(props: Props) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const toolsContainer = usePageToolsContainer()

  if (props.status === 'loading') {
    return <div className='grid place-items-center py-10'>
      <Spinner />
    </div>
  }
  if (props.status === 'failed') {
    return <p className='text-sm text-danger'>{t('common:errors.unexpected')}</p>
  }

  return <MeetingTable
    meetings={props.meetings}
    controlsContainer={toolsContainer}
    emptyNotice={<EmptyNotice>{
      props.hasSources
        ? t('list.empty')
        : <>
          <span>{t('list.noSources')}</span>
          {' '}
          <Link to={getTabUrl(UrlTree.settingsMeetings, 'sources')} className='text-link'>{
            t('list.connectSource')
          }</Link>
        </>
    }</EmptyNotice>}
  />
}
