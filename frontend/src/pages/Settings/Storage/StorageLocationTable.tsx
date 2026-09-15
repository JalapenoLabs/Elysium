// Copyright © 2026 Jalapeno Labs

import type { StorageLocation } from '../../../api/routes/storageRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { StorageLocationRowActions } from './StorageLocationRowActions'

// Misc
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import {
  bunnyRegionLabelKeys,
  formatStorageBytes,
  getStoragePath,
  storageProviderLabelKeys,
} from './storagePresentation'

type Props = {
  locations: StorageLocation[]
  onEdit: (location: StorageLocation) => void
  onTest: (location: StorageLocation) => void
  onDelete: (location: StorageLocation) => void
}

const STORAGE_COLUMN_KEYS = [ 'name', 'provider', 'location', 'limit', 'rowActions' ] as const
type StorageColumnKey = typeof STORAGE_COLUMN_KEYS[number]

const columnLabelKeys = {
  name: 'table.name',
  provider: 'table.provider',
  location: 'table.location',
  limit: 'table.limit',
  rowActions: 'common:actions.moreActions',
} as const satisfies Record<StorageColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  name: 240,
  provider: 220,
  location: 300,
  limit: 140,
  rowActions: 64,
} as const satisfies Record<StorageColumnKey, number>

export function StorageLocationTable(props: Props) {
  const { t, i18n } = useTranslation([ 'storage', 'common' ])
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    const renderCell = {
      name: (location: StorageLocation) => <span className='font-medium'>{location.name}</span>,
      provider: (location: StorageLocation) => <div>
        <div>{t(storageProviderLabelKeys[location.provider.kind])}</div>
        <div className='text-xs opacity-70'>{t(bunnyRegionLabelKeys[location.provider.region])}</div>
      </div>,
      location: (location: StorageLocation) => <code className='text-xs'>{getStoragePath(location)}</code>,
      limit: (location: StorageLocation) => formatStorageBytes(location.storageLimitBytes, i18n.language),
      rowActions: (location: StorageLocation) => <StorageLocationRowActions
        location={location}
        onEdit={props.onEdit}
        onTest={props.onTest}
        onDelete={props.onDelete}
      />,
    } satisfies Record<StorageColumnKey, (location: StorageLocation) => unknown>

    // Search matches what the user sees.
    const searchValue = {
      name: (location: StorageLocation) => location.name,
      provider: (location: StorageLocation) => [
        t(storageProviderLabelKeys[location.provider.kind]),
        t(bunnyRegionLabelKeys[location.provider.region]),
      ].join(' '),
      location: getStoragePath,
      limit: (location: StorageLocation) => formatStorageBytes(location.storageLimitBytes, i18n.language),
      rowActions: null,
    } satisfies Record<StorageColumnKey, ((location: StorageLocation) => string) | null>

    return createManagedColumns<StorageLocation, StorageColumnKey>({
      columnKeys: STORAGE_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: (columnKey) => searchValue[columnKey]
        ? columnKey
        : null,
      getSearchValue: (columnKey) => searchValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => {
        const toSearchText = searchValue[columnKey]
        if (!toSearchText) {
          return {
            id: columnId,
            header: () => <span className='sr-only'>{columnLabel}</span>,
            enableSorting: false,
            size: columnSizes[columnKey],
            cell: ({ row }) => renderCell[columnKey](row.original),
          }
        }

        // The limit sorts by size, not by its formatted text.
        if (columnKey === 'limit') {
          return {
            id: columnId,
            header: columnLabel,
            accessorFn: (location) => location.storageLimitBytes,
            size: columnSizes[columnKey],
            cell: ({ row }) => renderCell[columnKey](row.original),
          }
        }

        return {
          id: columnId,
          header: columnLabel,
          accessorFn: toSearchText,
          size: columnSizes[columnKey],
          cell: ({ row }) => renderCell[columnKey](row.original),
        }
      },
    })
  }, [ t, i18n.language, props.onEdit, props.onTest, props.onDelete ])

  if (!props.locations.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('table.empty')
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: 'storage-locations-table',
      tableLocalStorageId: 'elysium.settings.storage.table',
    }}
    tableAriaLabel={t('table.label')}
    data={props.locations}
    managedColumns={managedColumns}
    getRowId={(location) => location.id}
    labels={labels}
    search={{
      value: search,
      onChange: setSearch,
    }}
    enableSorting
    stickyHeader={false}
  />
}
