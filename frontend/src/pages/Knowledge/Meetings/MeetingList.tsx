// Copyright © 2026 Jalapeno Labs

import type { SmartTableFilters } from '@jalapenolabs/uikit'
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
import { UrlTree } from '../../../urls'

type Props = {
  status: LoadStatus
  // Newest first, as the filters leave them.
  meetings: Meeting[]
  // Whether any meeting source is connected. Without one, an empty list points to Settings,
  // Meetings, where one is connected.
  hasSources: boolean
  // Whether the filters leave out any meeting, so an empty list says they did.
  isFiltered: boolean
  filters: SmartTableFilters<Meeting>
}

// The Meetings page's list: the person's meetings as the page's one table, its search, filters,
// Columns, and export in the titlebar.
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

  // Nothing to list says why: the filters left nothing, no source has read a meeting yet, or there
  // is no source to read one.
  let emptyNotice = <EmptyNotice>{t('list.empty')}</EmptyNotice>
  if (props.isFiltered) {
    emptyNotice = <EmptyNotice>{t('list.noMatch')}</EmptyNotice>
  }
  else if (!props.hasSources) {
    emptyNotice = <EmptyNotice>
      <span>{t('list.noSources')}</span>
      {' '}
      <Link to={UrlTree.settingsMeetings} className='text-link'>{t('list.connectSource')}</Link>
    </EmptyNotice>
  }

  return <MeetingTable
    meetings={props.meetings}
    controlsContainer={toolsContainer}
    filters={props.filters}
    emptyNotice={emptyNotice}
  />
}
