// Copyright © 2026 Jalapeno Labs

import type { CsvValue } from '@jalapenolabs/uikit'
import type { WorkflowRun } from '../../api/routes/runRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { shallowEqual } from 'react-redux'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectUserNamesById } from '../../store/usersSlice'

// User interface
import { Chip, Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { RunDuration } from './RunDuration'

// Misc
import { DateText } from '../../format/DateText'
import { formatDateTime } from '../../format/dates'
import { formatDuration } from '../../format/durations'
import { useUsersLoader } from '../../hooks/useServerData'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import { getRunViewUrl } from '../../urls'
import { runStateChipColors, runStateLabelKeys } from './runPresentation'
import { useRunNames } from './useRunNames'

type Props = {
  runs: WorkflowRun[]
}

const RUN_COLUMN_KEYS = [ 'run', 'state', 'startedBy', 'started', 'duration' ] as const
type RunColumnKey = typeof RUN_COLUMN_KEYS[number]

const columnLabelKeys = {
  run: 'list.columns.run',
  state: 'list.columns.state',
  startedBy: 'list.columns.startedBy',
  started: 'list.columns.started',
  duration: 'list.columns.duration',
} as const satisfies Record<RunColumnKey, string>

// Starting widths in pixels, summing to less than the content column at md.
const columnSizes = {
  run: 320,
  state: 150,
  startedBy: 170,
  started: 190,
  duration: 120,
} as const satisfies Record<RunColumnKey, number>

// Every run loaded so far, newest first, each opening its own page.
export function RunTable(props: Props) {
  const { t } = useTranslation([ 'runs', 'common' ])
  const labels = useSmartTableLabels()
  const names = useRunNames()
  const [ search, setSearch ] = useState('')
  useUsersLoader()
  const userNames = useAppSelector(selectUserNamesById, shallowEqual)

  const managedColumns = useMemo(() => {
    const startedBy = (run: WorkflowRun) => userNames[run.createdBy] ?? t('common:table.someone')

    // How long a finished run took; one still going has no length yet.
    function elapsed(run: WorkflowRun) {
      if (!run.startedAt || !run.finishedAt) {
        return null
      }
      return Date.parse(run.finishedAt) - Date.parse(run.startedAt)
    }

    // Search matches what the user sees.
    const searchValue = {
      run: (run: WorkflowRun) => `${names.workflow(run.workflow)} ${names.source(run.sourceKind)} ${run.sourceId}`,
      state: (run: WorkflowRun) => t(runStateLabelKeys[run.state]),
      startedBy,
      started: (run: WorkflowRun) => formatDateTime(run.createdAt),
      duration: (run: WorkflowRun) => {
        const milliseconds = elapsed(run)
        return milliseconds === null
          ? ''
          : formatDuration(milliseconds)
      },
    } satisfies Record<RunColumnKey, (run: WorkflowRun) => string>

    // Sorting follows the text, except the start, which sorts in time, and the duration, which
    // sorts by length with a run still going longest.
    const sortValue = {
      ...searchValue,
      started: (run: WorkflowRun) => Date.parse(run.createdAt),
      duration: (run: WorkflowRun) => elapsed(run) ?? Number.POSITIVE_INFINITY,
    } satisfies Record<RunColumnKey, (run: WorkflowRun) => string | number>

    // The export names the workflow, and holds the instant it started.
    const exportValue = {
      ...searchValue,
      run: (run: WorkflowRun) => names.workflow(run.workflow),
      started: (run: WorkflowRun) => run.createdAt,
    } satisfies Record<RunColumnKey, (run: WorkflowRun) => CsvValue>

    const renderCell = {
      run: (run: WorkflowRun) => <div className='min-w-0'>
        <Link href={getRunViewUrl(run.id)} className='font-medium text-inherit no-underline'>{
          names.workflow(run.workflow)
        }</Link>
        <div className='text-xs opacity-70'>{
          t('run.source', { kind: names.source(run.sourceKind), id: run.sourceId })
        }</div>
      </div>,
      state: (run: WorkflowRun) => <Chip size='sm' variant='soft' color={runStateChipColors[run.state]}>{
        t(runStateLabelKeys[run.state])
      }</Chip>,
      startedBy,
      started: (run: WorkflowRun) => <DateText value={run.createdAt} isFocusable={false} />,
      duration: (run: WorkflowRun) => <RunDuration
        startedAt={run.startedAt}
        finishedAt={run.finishedAt}
        isLive={run.state === 'running'}
      />,
    } satisfies Record<RunColumnKey, (run: WorkflowRun) => unknown>

    return createManagedColumns<WorkflowRun, RunColumnKey>({
      columnKeys: RUN_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: (columnKey) => columnKey,
      getSearchValue: (columnKey) => searchValue[columnKey],
      getExportValue: (columnKey) => exportValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => ({
        id: columnId,
        header: columnLabel,
        accessorFn: sortValue[columnKey],
        size: columnSizes[columnKey],
        cell: ({ row }) => renderCell[columnKey](row.original),
      }),
    })
  }, [ t, names, userNames ])

  return <SmartTable
    ids={{
      tableElementId: 'runs-table',
      tableLocalStorageId: 'elysium.runs.list',
    }}
    tableAriaLabel={t('list.label')}
    data={props.runs}
    managedColumns={managedColumns}
    getRowId={(run) => run.id}
    labels={labels}
    search={{
      value: search,
      onChange: setSearch,
    }}
    csvExport={{ fileName: 'runs' }}
    pagination={{ show: true }}
    enableSorting
    stickyHeader={false}
  />
}
