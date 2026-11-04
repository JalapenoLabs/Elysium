// Copyright © 2026 Jalapeno Labs

import type { DatabaseBackup } from '../../../api/routes/backupRoutes'
import type { StorageLocation } from '../../../api/routes/storageRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'

// Misc
import { DateText } from '../../../format/DateText'
import { formatDateTime } from '../../../format/dates'
import { formatBytes } from '../../../format/numbers'
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import { statusChipColors, statusLabelKeys, triggerLabelKeys } from './backupsPresentation'

type Props = {
  backups: DatabaseBackup[]
  // To name where each backup went.
  locations: StorageLocation[]
}

const HISTORY_COLUMN_KEYS = [ 'started', 'trigger', 'status', 'location', 'size', 'error' ] as const
type HistoryColumnKey = typeof HISTORY_COLUMN_KEYS[number]

const columnLabelKeys = {
  started: 'history.started',
  trigger: 'history.trigger',
  status: 'history.status',
  location: 'history.location',
  size: 'history.size',
  error: 'history.error',
} as const satisfies Record<HistoryColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  started: 190,
  trigger: 150,
  status: 120,
  location: 170,
  size: 110,
  error: 260,
} as const satisfies Record<HistoryColumnKey, number>

// Every recent backup attempt, newest first.
export function BackupHistoryTable(props: Props) {
  const { t } = useTranslation('backups')
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    const locationNames = new Map(props.locations.map((location) => [ location.id, location.name ]))

    function locationName(backup: DatabaseBackup) {
      if (!backup.storageLocationId) {
        return ''
      }
      return locationNames.get(backup.storageLocationId) ?? ''
    }

    function sizeText(backup: DatabaseBackup) {
      if (backup.sizeBytes === null) {
        return ''
      }
      return formatBytes(backup.sizeBytes)
    }

    // Search matches what the user sees.
    const searchValue = {
      started: (backup: DatabaseBackup) => formatDateTime(backup.startedAt),
      trigger: (backup: DatabaseBackup) => t(triggerLabelKeys[backup.trigger]),
      status: (backup: DatabaseBackup) => t(statusLabelKeys[backup.status]),
      location: locationName,
      size: sizeText,
      error: (backup: DatabaseBackup) => backup.error ?? '',
    } satisfies Record<HistoryColumnKey, (backup: DatabaseBackup) => string>

    // Sorting follows the text, except the start, which sorts in time, and the size, which sorts
    // by bytes with an unfinished backup's missing size smallest.
    const sortValue = {
      ...searchValue,
      started: (backup: DatabaseBackup) => Date.parse(backup.startedAt),
      size: (backup: DatabaseBackup) => backup.sizeBytes ?? -1,
    } satisfies Record<HistoryColumnKey, (backup: DatabaseBackup) => string | number>

    const renderCell = {
      started: (backup: DatabaseBackup) => <DateText value={backup.startedAt} isFocusable={false} />,
      trigger: (backup: DatabaseBackup) => t(triggerLabelKeys[backup.trigger]),
      status: (backup: DatabaseBackup) => <Chip
        size='sm'
        variant='soft'
        color={statusChipColors[backup.status]}
      >{
          t(statusLabelKeys[backup.status])
        }</Chip>,
      location: locationName,
      size: sizeText,
      error: (backup: DatabaseBackup) => <span className='text-sm opacity-80'>{backup.error ?? ''}</span>,
    } satisfies Record<HistoryColumnKey, (backup: DatabaseBackup) => unknown>

    return createManagedColumns<DatabaseBackup, HistoryColumnKey>({
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
  }, [ t, props.locations ])

  if (!props.backups.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('history.empty')
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: 'backup-history-table',
      tableLocalStorageId: 'elysium.settings.backups.history',
    }}
    tableAriaLabel={t('history.label')}
    data={props.backups}
    managedColumns={managedColumns}
    getRowId={(backup) => backup.id}
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
