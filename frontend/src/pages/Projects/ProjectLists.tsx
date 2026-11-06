// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectProjectActionItems } from '../../store/actionItemsSlice'
import { selectIsCompanyMember } from '../../store/authSlice'
import { selectCodingSessionsByProjectId } from '../../store/codingSessionsSlice'
import { useAppSelector } from '../../store/hooks'
import { selectProjectInitiatives } from '../../store/initiativesSlice'
import { selectProjectManager } from '../../store/projectManagersSlice'

// User interface
import { Spinner, Tabs } from '@heroui/react'
import { PageTabs } from '../../components/PageTabs'
import { CodingSessionsTable } from '../Coding/CodingSessionsTable'
import { ProjectInitiatives } from './ProjectInitiatives'
import { ProjectItems } from './ProjectItems'
import { ProjectManagerPanel } from './ProjectManagerPanel'
import { ProjectPages } from './ProjectPages'
import { ProjectTimeline } from './ProjectTimeline'
import { PromisesTable } from './PromisesTable'
import { WorkBatchesTable } from './WorkBatchesTable'

// Misc
import {
  useActionItemsLoader,
  useCodingSessionsLoader,
  useInitiativesLoader,
  useProjectManagerLoader,
  useProjectTimelineLoader,
} from '../../hooks/useServerData'
import { useTabParam } from '../../hooks/useTabParam'
import { PROJECT_DOCS_TAB } from '../../urls'
import { DEFAULT_STATES } from '../ActionItems/actionItemFilters'
import { useProjectPages } from './useProjectPages'

const MEMBER_TABS = [
  'items',
  'initiatives',
  'timeline',
  PROJECT_DOCS_TAB,
  'manager',
  'proposed',
  'batches',
  'promises',
  'sessions',
] as const

const GUEST_TABS = [ 'items', 'initiatives', 'timeline', 'sessions' ] as const

type Props = {
  projectId: string
}

// A project's lists below what it is about, one per tab, each named with its total: the items
// still owed, its initiatives, its timeline, the knowledge base pages linked to it, its Project
// Manager's turns, the work batches waiting for a person's approval, its latest work batches, and
// the promises it keeps, and its coding sessions. A guest reads no knowledge base, and the Project
// Manager's tabs quote what was ingested, so only the company's members see those five. The open
// tab lives in the address, and switching never asks about the page's unsaved changes, which stay
// as they are.
export function ProjectLists(props: Props) {
  const { t } = useTranslation([ 'projects', 'timeline' ])
  const isMember = useAppSelector(selectIsCompanyMember)
  const [ selectedTab, selectTab ] = useTabParam(isMember
    ? MEMBER_TABS
    : GUEST_TABS)
  const itemsStatus = useActionItemsLoader()
  const initiativesStatus = useInitiativesLoader()
  const sessionsStatus = useCodingSessionsLoader()
  // The newest page, which the timeline also asks for; SWR shares the one request.
  const timeline = useProjectTimelineLoader(props.projectId, null)
  const projectItems = useAppSelector((state) => selectProjectActionItems(state, props.projectId))
  const initiatives = useAppSelector((state) => selectProjectInitiatives(state, props.projectId))
  const sessions = useAppSelector((state) => selectCodingSessionsByProjectId(state, props.projectId))
  const docs = useProjectPages(props.projectId, isMember)
  const managerStatus = useProjectManagerLoader(props.projectId, isMember)
  const manager = useAppSelector((state) => selectProjectManager(state, props.projectId))

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
      ...isMember
        ? [
          { id: PROJECT_DOCS_TAB, label: t('docs.tab'), count: docs.pages?.length ?? null },
          { id: 'manager', label: t('manager.tab'), count: null },
          {
            id: 'proposed',
            label: t('batches.proposedTab'),
            count: manager?.proposed.length ?? null,
            isWaiting: true,
          },
          { id: 'batches', label: t('batches.tab'), count: manager?.batches.length ?? null },
          { id: 'promises', label: t('promises.tab'), count: manager?.promises.length ?? null },
        ]
        : [],
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
    {isMember && <Tabs.Panel id={PROJECT_DOCS_TAB}>
      <ProjectPages projectId={props.projectId} status={docs.status} pages={docs.pages ?? []} />
    </Tabs.Panel>}
    {isMember && <Tabs.Panel id='manager'>
      <ProjectManagerPanel projectId={props.projectId} status={managerStatus} />
    </Tabs.Panel>}
    {isMember && <Tabs.Panel id='proposed'>{
      manager
        ? <WorkBatchesTable list='proposed' batches={manager.proposed} />
        : <div className='grid place-items-center py-10'>
          <Spinner />
        </div>
    }</Tabs.Panel>}
    {isMember && <Tabs.Panel id='batches'>{
      manager
        ? <WorkBatchesTable list='latest' batches={manager.batches} />
        : <div className='grid place-items-center py-10'>
          <Spinner />
        </div>
    }</Tabs.Panel>}
    {isMember && <Tabs.Panel id='promises'>{
      manager
        ? <PromisesTable promises={manager.promises} batches={manager.batches} />
        : <div className='grid place-items-center py-10'>
          <Spinner />
        </div>
    }</Tabs.Panel>}
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
