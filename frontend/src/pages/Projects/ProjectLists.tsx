// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectProjectActionItems } from '../../store/actionItemsSlice'
import { selectCodingSessionsByProjectId } from '../../store/codingSessionsSlice'
import { useAppSelector } from '../../store/hooks'
import { selectProjectInitiatives } from '../../store/initiativesSlice'

// User interface
import { Spinner, Tabs } from '@heroui/react'
import { PageTabs } from '../../components/PageTabs'
import { CodingSessionsTable } from '../Coding/CodingSessionsTable'
import { ProjectInitiatives } from './ProjectInitiatives'
import { ProjectItems } from './ProjectItems'
import { ProjectTimeline } from './ProjectTimeline'

// Misc
import {
  useActionItemsLoader,
  useCodingSessionsLoader,
  useInitiativesLoader,
  useProjectTimelineLoader,
} from '../../hooks/useServerData'
import { useTabParam } from '../../hooks/useTabParam'
import { DEFAULT_STATES } from '../ActionItems/actionItemFilters'

const TABS = [ 'items', 'initiatives', 'timeline', 'sessions' ] as const

type Props = {
  projectId: string
}

// A project's lists below what it is about, one per tab, each named with its total: the items
// still owed, its initiatives, its timeline, and its coding sessions. The open tab lives in the
// address, and switching never asks about the page's unsaved changes, which stay as they are.
export function ProjectLists(props: Props) {
  const { t } = useTranslation([ 'projects', 'timeline' ])
  const [ selectedTab, selectTab ] = useTabParam(TABS)
  const itemsStatus = useActionItemsLoader()
  const initiativesStatus = useInitiativesLoader()
  const sessionsStatus = useCodingSessionsLoader()
  // The newest page, which the timeline also asks for; SWR shares the one request.
  const timeline = useProjectTimelineLoader(props.projectId, null)
  const projectItems = useAppSelector((state) => selectProjectActionItems(state, props.projectId))
  const initiatives = useAppSelector((state) => selectProjectInitiatives(state, props.projectId))
  const sessions = useAppSelector((state) => selectCodingSessionsByProjectId(state, props.projectId))

  // What All items lists by default: the inbox and open items.
  const owedItems: ActionItem[] = []
  for (const item of projectItems) {
    if (DEFAULT_STATES.includes(item.state)) {
      owedItems.push(item)
    }
  }

  return <PageTabs
    label={t('lists.label')}
    selectedKey={selectedTab}
    onSelectionChange={selectTab}
    tabs={[
      {
        id: 'items',
        label: t('work.itemsHeading'),
        count: itemsStatus === 'loaded'
          ? owedItems.length
          : null,
      },
      {
        id: 'initiatives',
        label: t('work.initiativesHeading'),
        count: initiativesStatus === 'loaded'
          ? initiatives.length
          : null,
      },
      { id: 'timeline', label: t('timeline:title'), count: timeline.total },
      {
        id: 'sessions',
        label: t('page.sessionsHeading'),
        count: sessionsStatus === 'loaded'
          ? sessions.length
          : null,
      },
    ]}
  >
    <Tabs.Panel id='items'>
      <ProjectItems
        projectId={props.projectId}
        status={itemsStatus}
        owedItems={owedItems}
        itemCount={projectItems.length}
      />
    </Tabs.Panel>
    <Tabs.Panel id='initiatives'>
      <ProjectInitiatives
        projectId={props.projectId}
        status={initiativesStatus}
        initiatives={initiatives}
      />
    </Tabs.Panel>
    <Tabs.Panel id='timeline'>
      <ProjectTimeline projectId={props.projectId} />
    </Tabs.Panel>
    <Tabs.Panel id='sessions'>{
      sessionsStatus === 'loading'
        ? <div className='grid place-items-center py-10'>
          <Spinner />
        </div>
        : <CodingSessionsTable
          sessions={sessions}
          ids={{
            tableElementId: 'project-sessions-table',
            tableLocalStorageId: 'elysium.projects.sessions.table.v2',
          }}
          ariaLabel={t('page.sessionsHeading')}
          emptyMessage={t('page.sessionsEmpty')}
        />
    }</Tabs.Panel>
  </PageTabs>
}
