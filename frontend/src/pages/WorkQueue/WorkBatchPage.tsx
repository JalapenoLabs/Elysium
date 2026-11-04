// Copyright © 2026 Jalapeno Labs

import type { WorkBatch } from '../../api/routes/projectManagerRoutes'
import type { WorkBatchHistoryEntry } from '../../api/routes/workQueueRoutes'
import type { PageAction } from '../../layout/page/pageActions'
import type { PageCrumb } from '../../layout/page/PageHeader'

// Core
import { useTranslation } from 'react-i18next'
import { Navigate, useParams } from 'react-router'
import { shallowEqual } from 'react-redux'

// Redux
import { selectIsCompanyMember } from '../../store/authSlice'
import { useAppSelector } from '../../store/hooks'
import { selectProjectNamesById } from '../../store/projectsSlice'
import { selectWorkBatch } from '../../store/workQueueSlice'

// User interface
import { Chip, Link, Spinner } from '@heroui/react'
import { Page } from '../../layout/page/Page'
import { WorkBatchHistory } from './WorkBatchHistory'
import { WorkBatchPullRequests } from './WorkBatchPullRequests'
import { WorkerLog } from './WorkerLog'

// Misc
import { formatDateTime } from '../../format/dates'
import { useNow } from '../../hooks/useNow'
import { useProjectsLoader, useWorkBatchLoader } from '../../hooks/useServerData'
import { getProjectViewUrl, UrlTree } from '../../urls'
import {
  batchPriorityChipColors,
  batchPriorityLabelKeys,
  batchStateChipColors,
  batchStateLabelKeys,
} from '../Projects/projectManagerPresentation'
import { useWorkBatchActions } from './useWorkBatchActions'
import {
  actionIcons,
  actionLabelKeys,
  availableActions,
  currentWait,
  waitHeadingKeys,
  waitUntilKeys,
} from './workQueuePresentation'

// `/work-queue/:batchId`: one batch whole, what its worker waits on, its goal, the repositories it
// needs, how it ended, the pull requests its worker opened, its history, and its worker's log, with
// what may still be done to it in its Actions. A guest is sent to the start.
export function WorkBatchPage() {
  const isCompanyMember = useAppSelector(selectIsCompanyMember)
  const { batchId = '' } = useParams()

  if (!isCompanyMember) {
    return <Navigate to={UrlTree.root} replace />
  }
  return <WorkBatchView batchId={batchId} />
}

type ViewProps = {
  batchId: string
}

function WorkBatchView(props: ViewProps) {
  const { t } = useTranslation('workQueue')
  const status = useWorkBatchLoader(props.batchId, true)
  const loaded = useAppSelector((state) => selectWorkBatch(state, props.batchId))
  const parents = [{ label: t('title'), href: UrlTree.workQueue }]

  if (!loaded) {
    return <Page title={t('title')} parents={parents}>{
      status === 'failed'
        ? <p className='py-10 text-center text-sm text-danger'>{t('batch.loadError')}</p>
        : <div className='grid place-items-center py-16'>
          <Spinner />
        </div>
    }</Page>
  }
  return <WorkBatchDetails batch={loaded.batch} history={loaded.history} parents={parents} />
}

type DetailsProps = {
  batch: WorkBatch
  history: WorkBatchHistoryEntry[]
  parents: PageCrumb[]
}

function WorkBatchDetails(props: DetailsProps) {
  const { t } = useTranslation('workQueue')
  const projectsTranslation = useTranslation('projects')
  const act = useWorkBatchActions()
  const now = useNow()
  useProjectsLoader()
  const projectNames = useAppSelector(selectProjectNamesById, shallowEqual)
  const batch = props.batch
  const wait = currentWait(batch)

  const actions: PageAction[] = availableActions(batch, now).map((action) => ({
    id: action,
    label: t(actionLabelKeys[action]),
    icon: actionIcons[action],
    isDanger: action === 'cancel',
    onAction: () => void act(batch, action),
  }))

  return <Page
    title={batch.title}
    parents={props.parents}
    badges={<>
      <Chip variant='soft' color={batchStateChipColors[batch.state]}>{
        projectsTranslation.t(batchStateLabelKeys[batch.state])
      }</Chip>
      {batch.pinnedAt && <Chip variant='soft' color='accent'>{t('table.runsNext')}</Chip>}
    </>}
    actions={actions}
  >
    <dl className='relaxed grid max-w-3xl grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm'>
      <dt className='opacity-70'>{t('batch.project')}</dt>
      <dd>
        <Link href={getProjectViewUrl(batch.projectId)} className='text-link'>{
          projectNames[batch.projectId] ?? ''
        }</Link>
      </dd>
      <dt className='opacity-70'>{t('batch.priority')}</dt>
      <dd className='flex items-center gap-2'>
        <Chip size='sm' variant='soft' color={batchPriorityChipColors[batch.priority]}>{
          projectsTranslation.t(batchPriorityLabelKeys[batch.priority])
        }</Chip>
        {batch.scheduledFor && <span className='opacity-80'>{formatDateTime(batch.scheduledFor)}</span>}
      </dd>
      <dt className='opacity-70'>{t('batch.submitted')}</dt>
      <dd>{formatDateTime(batch.createdAt)}</dd>
      {batch.startedAt && <>
        <dt className='opacity-70'>{t('batch.started')}</dt>
        <dd>{formatDateTime(batch.startedAt)}</dd>
      </>}
      {batch.finishedAt && <>
        <dt className='opacity-70'>{t('batch.finished')}</dt>
        <dd>{formatDateTime(batch.finishedAt)}</dd>
      </>}
    </dl>

    {wait && <section className='relaxed max-w-3xl'>
      <h2 className='compact text-lg font-semibold'>{t(waitHeadingKeys[wait.on])}</h2>
      {wait.detail && <p className='compact whitespace-pre-wrap text-sm'>{wait.detail}</p>}
      {wait.until && <p className='text-sm opacity-70'>{
        t(waitUntilKeys[wait.on], { time: formatDateTime(wait.until) })
      }</p>}
    </section>}

    {batch.result && <section className='relaxed max-w-3xl'>
      <h2 className='compact text-lg font-semibold'>{
        batch.state === 'cancelled'
          ? t('batch.cancelledBecause')
          : t('batch.result')
      }</h2>
      <p className='whitespace-pre-wrap text-sm'>{batch.result}</p>
    </section>}

    <section className='relaxed max-w-3xl'>
      <h2 className='compact text-lg font-semibold'>{t('batch.goal')}</h2>
      <p className='whitespace-pre-wrap text-sm'>{batch.goal}</p>
    </section>

    <section className='relaxed max-w-3xl'>
      <h2 className='compact text-lg font-semibold'>{t('batch.repositories')}</h2>
      {batch.repositories.length
        ? <ul className='flex flex-col gap-1 text-sm'>{
          batch.repositories.map((repository) => <li key={repository.url}>
            <span className='font-mono'>{repository.url}</span>
            {repository.baseBranch && <span className='ml-2 opacity-70'>{
              t('batch.fromBranch', { branch: repository.baseBranch })
            }</span>}
          </li>)
        }</ul>
        : <p className='text-sm opacity-70'>{t('batch.noRepositories')}</p>}
    </section>

    {batch.startedAt && <WorkBatchPullRequests batchId={batch.id} />}

    <section className='relaxed max-w-3xl'>
      <h2 className='compact text-lg font-semibold'>{t('batch.history')}</h2>
      <WorkBatchHistory history={props.history} />
    </section>

    {batch.startedAt && <WorkerLog
      batchId={batch.id}
      startsOpen={batch.state === 'running'}
    />}
  </Page>
}
