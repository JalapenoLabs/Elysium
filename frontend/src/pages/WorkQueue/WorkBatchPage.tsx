// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { WorkBatch, WorkBatchState } from '../../api/routes/projectManagerRoutes'
import type { WorkBatchHistoryEntry, WorkBatchWhy as Why } from '../../api/routes/workQueueRoutes'
import type { PageAction } from '../../layout/page/pageActions'
import type { PageCrumb } from '../../layout/page/PageHeader'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Navigate, useParams } from 'react-router'
import { shallowEqual } from 'react-redux'

// Redux
import { selectIsCompanyMember } from '../../store/authSlice'
import { useAppSelector } from '../../store/hooks'
import { selectProjectNamesById } from '../../store/projectsSlice'
import { selectWorkBatch, selectWorkQueue } from '../../store/workQueueSlice'

// User interface
import { Button, Chip, Link, Spinner } from '@heroui/react'
import { LuPencil } from 'react-icons/lu'
import { ConfigurationHoldAlert } from '../../components/ConfigurationHoldAlert'
import { Page } from '../../layout/page/Page'
import { WorkBatchConfiguration } from './WorkBatchConfiguration'
import { WorkBatchHistory } from './WorkBatchHistory'
import { WorkBatchPullRequests } from './WorkBatchPullRequests'
import { WorkBatchWhy } from './WorkBatchWhy'
import { WorkerLog } from './WorkerLog'

// Misc
import { formatDateTime } from '../../format/dates'
import { formatNumber } from '../../format/numbers'
import { useNow } from '../../hooks/useNow'
import { useProjectsLoader, useWorkBatchLoader, useWorkQueueLoader } from '../../hooks/useServerData'
import { getProjectViewUrl, getWorkBatchEditUrl, getWorkBatchViewUrl, UrlTree } from '../../urls'
import {
  batchOriginChipColors,
  batchOriginLabelKeys,
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
  DANGEROUS_ACTIONS,
  waitHeadingKeys,
  waitUntilKeys,
} from './workQueuePresentation'

// `/work-queue/:batchId`: one batch whole, whether it traces to a person, how many batches led to
// it and which, what its worker waits on, or why a queued one waits for the configuration
// repository, its goal, the repositories it needs, why it exists, how it ended, the configuration
// its worker runs on, the pull requests its worker opened, its history, and its worker's log, with
// what may still be done to it in its Actions. A proposed batch leads with Approve, and offers
// Edit, which approves it once changed, and Reject. A guest is sent to the start.
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
  return <WorkBatchDetails
    batch={loaded.batch}
    why={loaded.why}
    history={loaded.history}
    parents={parents}
  />
}

type DetailsProps = {
  batch: WorkBatch
  why: Why
  history: WorkBatchHistoryEntry[]
  parents: PageCrumb[]
}

// The heading over a batch's result: what its worker reported, or why it was withdrawn or
// declined.
const resultHeadingKeys = {
  proposed: 'batch.result',
  queued: 'batch.result',
  paused: 'batch.result',
  assigned: 'batch.result',
  running: 'batch.result',
  succeeded: 'batch.result',
  failed: 'batch.result',
  cancelled: 'batch.cancelledBecause',
  rejected: 'batch.rejectedBecause',
} as const satisfies Record<WorkBatchState, ParseKeys<'workQueue'>>

function WorkBatchDetails(props: DetailsProps) {
  const { t } = useTranslation('workQueue')
  const projectsTranslation = useTranslation('projects')
  const act = useWorkBatchActions()
  const now = useNow()
  useProjectsLoader()
  const projectNames = useAppSelector(selectProjectNamesById, shallowEqual)
  const [ isApproving, setIsApproving ] = useState(false)
  const batch = props.batch
  const wait = currentWait(batch)
  const isProposed = batch.state === 'proposed'
  // A queued batch waits on the pool, which says why it hands no slot out.
  const isQueued = batch.state === 'queued'
  useWorkQueueLoader(isQueued)
  const queue = useAppSelector(selectWorkQueue)
  const configurationHold = isQueued
    ? queue?.pool.configurationHold
    : null

  // A proposal's Approve stands on its own as the page's lead; Edit, a page of its own that
  // approves it once changed, sits with Reject in the Actions.
  const actions: PageAction[] = []
  if (isProposed) {
    actions.push({
      id: 'edit',
      label: t('actions.edit'),
      icon: LuPencil,
      href: getWorkBatchEditUrl(batch.id),
    })
  }
  for (const action of availableActions(batch, now)) {
    if (action === 'approve') {
      continue
    }
    actions.push({
      id: action,
      label: t(actionLabelKeys[action]),
      icon: actionIcons[action],
      isDanger: DANGEROUS_ACTIONS.includes(action),
      onAction: () => void act(batch, action),
    })
  }

  async function approve() {
    setIsApproving(true)
    try {
      await act(batch, 'approve')
    }
    finally {
      setIsApproving(false)
    }
  }

  return <Page
    title={batch.title}
    parents={props.parents}
    badges={<>
      <Chip variant='soft' color={batchStateChipColors[batch.state]}>{
        projectsTranslation.t(batchStateLabelKeys[batch.state])
      }</Chip>
      {batch.pinnedAt && <Chip variant='soft' color='accent'>{t('table.runsNext')}</Chip>}
    </>}
    primaryAction={isProposed && <Button isPending={isApproving} onPress={() => void approve()}>
      <actionIcons.approve className='size-4' aria-hidden />
      <span>{t(actionLabelKeys.approve)}</span>
    </Button>}
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
      <dt className='opacity-70'>{t('batch.origin')}</dt>
      <dd>
        <Chip size='sm' variant='soft' color={batchOriginChipColors[batch.origin]}>{
          projectsTranslation.t(batchOriginLabelKeys[batch.origin])
        }</Chip>
      </dd>
      <dt className='opacity-70'>{t('batch.generation')}</dt>
      <dd className='tabular-nums'>{formatNumber(batch.generation)}</dd>
      {props.why.parents.length > 0 && <>
        <dt className='opacity-70'>{t('batch.plannedFrom')}</dt>
        <dd className='flex flex-col gap-1'>{
          props.why.parents.map((parent) => <Link
            key={parent.id}
            href={getWorkBatchViewUrl(parent.id)}
            className='text-link'
          >{
              parent.title
            }</Link>)
        }</dd>
      </>}
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

    {configurationHold && <ConfigurationHoldAlert hold={configurationHold} />}

    {wait && <section className='relaxed max-w-3xl'>
      <h2 className='compact text-lg font-semibold'>{t(waitHeadingKeys[wait.on])}</h2>
      {wait.detail && <p className='compact whitespace-pre-wrap text-sm'>{wait.detail}</p>}
      {wait.until && <p className='text-sm opacity-70'>{
        t(waitUntilKeys[wait.on], { time: formatDateTime(wait.until) })
      }</p>}
    </section>}

    {batch.result && <section className='relaxed max-w-3xl'>
      <h2 className='compact text-lg font-semibold'>{t(resultHeadingKeys[batch.state])}</h2>
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
            <span className='break-all font-mono'>{repository.url}</span>
            {repository.baseBranch && <span className='ml-2 opacity-70'>{
              t('batch.fromBranch', { branch: repository.baseBranch })
            }</span>}
          </li>)
        }</ul>
        : <p className='text-sm opacity-70'>{t('batch.noRepositories')}</p>}
    </section>

    <section className='relaxed max-w-3xl'>
      <h2 className='compact text-lg font-semibold'>{t('batch.why.title')}</h2>
      <WorkBatchWhy why={props.why} />
    </section>

    {batch.configuration && <WorkBatchConfiguration configuration={batch.configuration} />}

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
