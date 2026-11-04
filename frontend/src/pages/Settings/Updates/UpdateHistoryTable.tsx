// Copyright © 2026 Jalapeno Labs

import type { DeploymentUpdate } from '../../../api/routes/updateRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'

// Misc
import { DateText } from '../../../format/DateText'
import { formatDateTime } from '../../../format/dates'
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import { historyStatusChipColors, historyStatusLabelKeys } from './updatesPresentation'

type Props = {
  updates: DeploymentUpdate[]
}

const HISTORY_COLUMN_KEYS = [ 'release', 'status', 'started', 'finished', 'reason' ] as const
type HistoryColumnKey = typeof HISTORY_COLUMN_KEYS[number]

const columnLabelKeys = {
  release: 'history.release',
  status: 'history.status',
  started: 'history.started',
  finished: 'history.finished',
  reason: 'history.reason',
} as const satisfies Record<HistoryColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  release: 180,
  status: 140,
  started: 190,
  finished: 190,
  reason: 320,
} as const satisfies Record<HistoryColumnKey, number>

// Every update the updater applied or rolled back, newest first.
export function UpdateHistoryTable(props: Props) {
  const { t } = useTranslation('updates')
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    // Search matches what the user sees.
    const searchValue = {
      release: (update: DeploymentUpdate) => `${update.fromVersion} ${update.toVersion}`,
      status: (update: DeploymentUpdate) => t(historyStatusLabelKeys[update.status]),
      started: (update: DeploymentUpdate) => formatDateTime(update.startedAt),
      finished: (update: DeploymentUpdate) => update.finishedAt
        ? formatDateTime(update.finishedAt)
        : '',
      reason: (update: DeploymentUpdate) => update.reason ?? '',
    } satisfies Record<HistoryColumnKey, (update: DeploymentUpdate) => string>

    // Sorting follows the text, except the dates, which sort in time with one still going last.
    const sortValue = {
      ...searchValue,
      started: (update: DeploymentUpdate) => Date.parse(update.startedAt),
      finished: (update: DeploymentUpdate) => update.finishedAt
        ? Date.parse(update.finishedAt)
        : Number.POSITIVE_INFINITY,
    } satisfies Record<HistoryColumnKey, (update: DeploymentUpdate) => string | number>

    const renderCell = {
      release: (update: DeploymentUpdate) => t('history.fromTo', {
        from: update.fromVersion,
        to: update.toVersion,
      }),
      status: (update: DeploymentUpdate) => <Chip
        size='sm'
        variant='soft'
        color={historyStatusChipColors[update.status]}
      >{
          t(historyStatusLabelKeys[update.status])
        }</Chip>,
      started: (update: DeploymentUpdate) => <DateText value={update.startedAt} isFocusable={false} />,
      finished: (update: DeploymentUpdate) => update.finishedAt && <DateText
        value={update.finishedAt}
        isFocusable={false}
      />,
      reason: (update: DeploymentUpdate) => <span className='text-sm opacity-80'>{update.reason ?? ''}</span>,
    } satisfies Record<HistoryColumnKey, (update: DeploymentUpdate) => unknown>

    return createManagedColumns<DeploymentUpdate, HistoryColumnKey>({
      columnKeys: HISTORY_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: (columnKey) => columnKey,
      getSearchValue: (columnKey) => searchValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => ({
        id: columnId,
        header: columnLabel,
        accessorFn: sortValue[columnKey],
        size: columnSizes[columnKey],
        cell: ({ row }) => renderCell[columnKey](row.original),
      }),
    })
  }, [ t ])

  if (!props.updates.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('history.empty')
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: 'update-history-table',
      tableLocalStorageId: 'elysium.settings.updates.history',
    }}
    tableAriaLabel={t('history.label')}
    data={props.updates}
    managedColumns={managedColumns}
    getRowId={(update) => update.id}
    labels={labels}
    search={{
      value: search,
      onChange: setSearch,
    }}
    pagination={{ show: true }}
    enableSorting
    stickyHeader={false}
  />
}
