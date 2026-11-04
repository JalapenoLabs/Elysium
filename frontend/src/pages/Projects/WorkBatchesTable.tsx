// Copyright © 2026 Jalapeno Labs

import type { WorkBatch } from '../../api/routes/projectManagerRoutes'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'

// Misc
import { DateText } from '../../format/DateText'
import { formatShortDateTime } from '../../format/dates'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import {
  batchPriorityChipColors,
  batchPriorityLabelKeys,
  batchStateChipColors,
  batchStateLabelKeys,
} from './projectManagerPresentation'

type Props = {
  batches: WorkBatch[]
}

const BATCH_COLUMN_KEYS = [ 'title', 'priority', 'state', 'submitted', 'result' ] as const
type BatchColumnKey = typeof BATCH_COLUMN_KEYS[number]

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
// when it should run, where it stands, and what its worker reported.
export function WorkBatchesTable(props: Props) {
  const { t } = useTranslation('projects')
  const labels = useSmartTableLabels()

  const managedColumns = useMemo(() => {
    function priorityLabel(batch: WorkBatch) {
      return t(batchPriorityLabelKeys[batch.priority])
    }
    function stateLabel(batch: WorkBatch) {
      return t(batchStateLabelKeys[batch.state])
    }

    const renderCell = {
      title: (batch: WorkBatch) => <span className='font-medium'>{batch.title}</span>,
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
      columnKeys: BATCH_COLUMN_KEYS,
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
  }, [ t ])

  if (!props.batches.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('batches.empty')
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: 'project-work-batches-table',
      tableLocalStorageId: 'elysium.projects.batches.table.v1',
    }}
    tableAriaLabel={t('batches.label')}
    data={props.batches}
    managedColumns={managedColumns}
    getRowId={(batch) => batch.id}
    labels={labels}
    toolbar={{ show: false }}
    csvExport={{ fileName: 'work-batches' }}
    pagination={{ show: true }}
    enableSorting
    stickyHeader={false}
  />
}
