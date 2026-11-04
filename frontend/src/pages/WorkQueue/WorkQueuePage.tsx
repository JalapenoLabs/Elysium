// Copyright © 2026 Jalapeno Labs

import type { PageAction } from '../../layout/page/pageActions'
import type { WorkQueue } from '../../api/routes/workQueueRoutes'
import type { WorkQueueList } from './WorkQueueTable'

// Core
import { useTranslation } from 'react-i18next'
import { Navigate } from 'react-router'

// Redux
import { selectIsAdmin, selectIsCompanyMember } from '../../store/authSlice'
import { useAppSelector } from '../../store/hooks'
import { selectWorkQueue } from '../../store/workQueueSlice'

// User interface
import { Spinner, Tabs } from '@heroui/react'
import { LuSlidersHorizontal } from 'react-icons/lu'
import { PageTabs } from '../../components/PageTabs'
import { Page } from '../../layout/page/Page'
import { WorkerPoolSummary } from './WorkerPoolSummary'
import { WorkQueueTable } from './WorkQueueTable'

// Misc
import { useWorkQueueLoader } from '../../hooks/useServerData'
import { useTabParam } from '../../hooks/useTabParam'
import { UrlTree } from '../../urls'

const TABS = [ 'queued', 'running', 'scheduled', 'paused' ] as const satisfies readonly WorkQueueList[]

const tabLabelKeys = {
  queued: 'tabs.queued',
  running: 'tabs.running',
  scheduled: 'tabs.scheduled',
  paused: 'tabs.paused',
} as const satisfies Record<WorkQueueList, string>

// Which part of the queue each tab lists.
const entriesByList = {
  queued: (queue: WorkQueue) => queue.next,
  running: (queue: WorkQueue) => queue.inSlots,
  scheduled: (queue: WorkQueue) => queue.scheduled,
  paused: (queue: WorkQueue) => queue.paused,
} as const satisfies Record<WorkQueueList, (queue: WorkQueue) => WorkQueue['next']>

// `/work-queue`: the worker pool and every Project Manager's batch waiting for it or in it, the
// queue in the order slots are handed out. A batch's title quotes what was ingested, so a guest,
// whose sidebar has no link here, is sent back to the start.
export function WorkQueuePage() {
  const isCompanyMember = useAppSelector(selectIsCompanyMember)

  if (!isCompanyMember) {
    return <Navigate to={UrlTree.root} replace />
  }
  return <WorkQueueView />
}

function WorkQueueView() {
  const { t } = useTranslation('workQueue')
  const status = useWorkQueueLoader(true)
  const queue = useAppSelector(selectWorkQueue)
  const isAdmin = useAppSelector(selectIsAdmin)
  const [ selectedTab, selectTab ] = useTabParam(TABS)

  // Admins size the pool on its settings page.
  const actions: PageAction[] = isAdmin
    ? [{ id: 'poolSize', label: t('pool.size'), icon: LuSlidersHorizontal, href: UrlTree.settingsWorkerPool }]
    : []

  return <Page title={t('title')} help={t('help')} actions={actions}>
    {!queue && status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
      t('loadError')
    }</p>}
    {!queue && status !== 'failed' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}
    {queue && <>
      <WorkerPoolSummary pool={queue.pool} />
      <PageTabs
        label={t('tabs.label')}
        selectedKey={selectedTab}
        onSelectionChange={selectTab}
        tabs={TABS.map((tab) => ({
          id: tab,
          label: t(tabLabelKeys[tab]),
          count: entriesByList[tab](queue).length,
        }))}
      >{
          TABS.map((tab) => <Tabs.Panel key={tab} id={tab}>
            <WorkQueueTable list={tab} entries={entriesByList[tab](queue)} />
          </Tabs.Panel>)
        }</PageTabs>
    </>}
  </Page>
}
