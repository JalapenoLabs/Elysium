// Copyright © 2026 Jalapeno Labs

import type { DataTableAction } from '@jalapenolabs/uikit'
import type { WorkBatch } from '../../api/routes/projectManagerRoutes'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip, Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'

// Misc
import { DateText } from '../../format/DateText'
import { formatShortDateTime } from '../../format/dates'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import { getWorkBatchViewUrl } from '../../urls'
import { useWorkBatchActions } from '../WorkQueue/useWorkBatchActions'
import { actionIcons, actionLabelKeys, DANGEROUS_ACTIONS } from '../WorkQueue/workQueuePresentation'
import {
  batchPriorityChipColors,
  batchPriorityLabelKeys,
  batchStateChipColors,
  batchStateLabelKeys,
} from './projectManagerPresentation'

// Which of a project's batches a table lists: its latest, whatever their state, or those waiting
// for a person's approval.
export type WorkBatchesList = 'latest' | 'proposed'

type Props = {
  list: WorkBatchesList
  batches: WorkBatch[]
}

const BATCH_COLUMN_KEYS = [ 'title', 'priority', 'state', 'submitted', 'result' ] as const
type BatchColumnKey = typeof BATCH_COLUMN_KEYS[number]

// A proposal has no result yet, and every one is proposed, so those columns say nothing.
const columnKeysByList = {
  latest: BATCH_COLUMN_KEYS,
  proposed: [ 'title', 'priority', 'submitted' ],
} as const satisfies Record<WorkBatchesList, readonly BatchColumnKey[]>

// The latest list keeps the key it always had, so saved column layouts stay.
const tableStorageIds = {
  latest: 'elysium.projects.batches.table.v1',
  proposed: 'elysium.projects.batches.proposed.table.v1',
} as const satisfies Record<WorkBatchesList, string>

const emptyKeysByList = {
  latest: 'batches.empty',
  proposed: 'batches.proposedEmpty',
} as const satisfies Record<WorkBatchesList, string>

// What a row may do: a proposal is approved or rejected from here as from the work queue; the
// rest are changed from the work queue or their own page.
const rowActionsByList = {
  latest: [],
  proposed: [ 'approve', 'reject' ],
} as const satisfies Record<WorkBatchesList, readonly ('approve' | 'reject')[]>

const columnLabelKeys = {
  title: 'batches.columns.title',
  priority: 'batches.columns.priority',
  state: 'batches.columns.state',
  submitted: 'batches.columns.submitted',
  result: 'batches.columns.result',
} as const satisfies Record<BatchColumnKey, string>

// Starting widths in pixels, summing to less than the page's content column.
const columnSizes = {
  title: 260,
  priority: 150,
  state: 120,
  submitted: 130,
  result: 260,
} as const satisfies Record<BatchColumnKey, number>

// The batches a project's Project Manager submitted for workers, newest first: what each is,
// when it should run, where it stands, and what its worker reported, each title opening the
// batch's own page. Its proposals list on their own, each approved or rejected from its row.
export function WorkBatchesTable(props: Props) {
  const { t } = useTranslation('projects')
  const workQueueTranslation = useTranslation('workQueue')
  const labels = useSmartTableLabels()
  const act = useWorkBatchActions()

  const managedColumns = useMemo(() => {
    function priorityLabel(batch: WorkBatch) {
      return t(batchPriorityLabelKeys[batch.priority])
    }
    function stateLabel(batch: WorkBatch) {
      return t(batchStateLabelKeys[batch.state])
    }

    const renderCell = {
      title: (batch: WorkBatch) => <Link
        href={getWorkBatchViewUrl(batch.id)}
        className='font-medium text-inherit no-underline'
      >{
          batch.title
        }</Link>,
      // A scheduled batch names its time beneath its priority.
      priority: (batch: WorkBatch) => <div className='flex flex-col items-start gap-1'>
        <Chip size='sm' variant='soft' color={batchPriorityChipColors[batch.priority]}>{
          priorityLabel(batch)
        }</Chip>
        {batch.scheduledFor && <span className='text-xs opacity-70'>{
          formatShortDateTime(batch.scheduledFor)
        }</span>}
      </div>,
      state: (batch: WorkBatch) => <Chip size='sm' variant='soft' color={batchStateChipColors[batch.state]}>{
        stateLabel(batch)
      }</Chip>,
      submitted: (batch: WorkBatch) => <DateText value={batch.createdAt} isFocusable={false} />,
      result: (batch: WorkBatch) => <span className='text-sm opacity-80'>{batch.result ?? ''}</span>,
    } satisfies Record<BatchColumnKey, (batch: WorkBatch) => unknown>

    // Each sorts by its value, which is also what the export holds: the instant it was submitted.
    const sortValue = {
      title: (batch: WorkBatch) => batch.title,
      priority: priorityLabel,
      state: stateLabel,
      submitted: (batch: WorkBatch) => batch.createdAt,
      result: (batch: WorkBatch) => batch.result ?? '',
    } satisfies Record<BatchColumnKey, (batch: WorkBatch) => string>

    return createManagedColumns<WorkBatch, BatchColumnKey>({
      columnKeys: columnKeysByList[props.list],
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: () => null,
      getSearchValue: (columnKey) => sortValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => ({
        id: columnId,
        header: columnLabel,
        accessorFn: sortValue[columnKey],
        size: columnSizes[columnKey],
        cell: ({ row }) => renderCell[columnKey](row.original),
      }),
    })
  }, [ t, props.list ])

  const rowActions = useMemo((): DataTableAction<WorkBatch>[] => rowActionsByList[props.list].map((action) => {
    const Icon = actionIcons[action]
    return {
      id: action,
      label: workQueueTranslation.t(actionLabelKeys[action]),
      icon: <Icon className='size-4' aria-hidden />,
      isDanger: DANGEROUS_ACTIONS.includes(action),
      onPress: (row) => act(row.original, action),
    }
  }), [ props.list, workQueueTranslation.t, act ])

  if (!props.batches.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t(emptyKeysByList[props.list])
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: `project-work-batches-${props.list}-table`,
      tableLocalStorageId: tableStorageIds[props.list],
    }}
    tableAriaLabel={t('batches.label')}
    data={props.batches}
    managedColumns={managedColumns}
    getRowId={(batch) => batch.id}
    labels={labels}
    rowActions={rowActions}
    toolbar={{ show: false }}
    csvExport={{ fileName: 'work-batches' }}
    pagination={{ show: true }}
    enableSorting
    stickyHeader={false}
  />
}
