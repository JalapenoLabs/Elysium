// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { Navigate, useParams } from 'react-router'

// Redux
import { selectIsCompanyMember } from '../../store/authSlice'
import { useAppSelector } from '../../store/hooks'
import { selectRunById, selectRunHistory, selectStepsOfRun } from '../../store/runsSlice'

// User interface
import { Breadcrumbs, Chip, Link, Spinner } from '@heroui/react'
import { RunDuration } from './RunDuration'
import { RunHistory } from './RunHistory'
import { RunStepRow } from './RunStepRow'

// Misc
import { useRunLoader } from '../../hooks/useServerData'
import { useUserNames } from '../../hooks/useUserNames'
import { UrlTree } from '../../urls'
import { nameFromIdentifier, runStateChipColors, runStateLabelKeys } from './runPresentation'

const dateTimeFormatter = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' })

// `/runs/:runId`: one run as GitHub Actions shows one, its steps in order, each collapsed to a
// line that opens to its log, then its history. A guest is sent back to the start.
export function RunPage() {
  const isCompanyMember = useAppSelector(selectIsCompanyMember)
  const { runId = '' } = useParams()

  if (!isCompanyMember) {
    return <Navigate to={UrlTree.root} replace />
  }
  return <RunView runId={runId} />
}

type Props = {
  runId: string
}

function RunView(props: Props) {
  const { t } = useTranslation([ 'runs', 'common' ])
  const status = useRunLoader(props.runId)
  const userNames = useUserNames()
  const run = useAppSelector((state) => selectRunById(state, props.runId))
  const steps = useAppSelector((state) => selectStepsOfRun(state, props.runId))
  const history = useAppSelector((state) => selectRunHistory(state, props.runId))

  if (!run) {
    return <div className='container'>{
      status === 'failed'
        ? <div className='py-16 text-center'>
          <p className='compact text-sm opacity-70'>{t('run.loadError')}</p>
          <Link href={UrlTree.runs} className='text-link'>{t('title')}</Link>
        </div>
        : <div className='grid place-items-center py-16'>
          <Spinner />
        </div>
    }</div>
  }

  const name = nameFromIdentifier(run.workflow)
  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.runs}>{t('title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{name}</Breadcrumbs.Item>
    </Breadcrumbs>

    <div className='level relaxed items-start gap-4'>
      <div className='min-w-0 flex-1'>
        <h1 className='text-3xl font-bold'>{name}</h1>
        <p className='mt-1 text-sm opacity-70'>{
          t('run.source', { kind: nameFromIdentifier(run.sourceKind), id: run.sourceId })
        }</p>
        <p className='text-sm opacity-70'>{
          t('run.startedBy', {
            name: userNames[run.createdBy] ?? t('common:table.someone'),
            date: dateTimeFormatter.format(new Date(run.createdAt)),
          })
        }</p>
      </div>
      <div className='level-right shrink-0 gap-3'>
        <span className='text-sm opacity-80'>
          <RunDuration startedAt={run.startedAt} finishedAt={run.finishedAt} isLive={run.state === 'running'} />
        </span>
        <Chip variant='soft' color={runStateChipColors[run.state]}>{
          t(runStateLabelKeys[run.state])
        }</Chip>
      </div>
    </div>

    {run.error && <p className='relaxed rounded-lg border border-danger/40 px-4 py-3 text-sm text-danger'>{
      run.error
    }</p>}

    <section className='relaxed'>
      <h2 className='compact text-lg font-semibold'>{t('run.steps')}</h2>
      <ol>{
        steps.map((step) => <RunStepRow key={step.id} step={step} />)
      }</ol>
    </section>

    {history.length > 0 && <section className='relaxed'>
      <h2 className='compact text-lg font-semibold'>{t('run.history')}</h2>
      <RunHistory history={history} steps={steps} />
    </section>}
  </div>
}
