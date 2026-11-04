// Copyright © 2026 Jalapeno Labs

import type { DataTableAction } from '@jalapenolabs/uikit'
import type { StorageLocation } from '../../../api/routes/storageRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { shallowEqual } from 'react-redux'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectProjectNamesById } from '../../../store/projectsSlice'

// User interface
import { Chip, Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuPlugZap, LuTrash2 } from 'react-icons/lu'

// Misc
import { ALL_PROJECTS } from '../../../api/routes/projectRoutes'
import { formatBytes } from '../../../format/numbers'
import { useProjectsLoader } from '../../../hooks/useServerData'
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import { useUserNames } from '../../../hooks/useUserNames'
import { getStorageLocationEditUrl } from '../../../urls'
import {
  bunnyRegionLabelKeys,
  getStorageOption,
  getStoragePath,
  storageOptionLabelKeys,
} from './storagePresentation'

type Props = {
  locations: StorageLocation[]
  onTest: (location: StorageLocation) => Promise<void>
  onDelete: (location: StorageLocation) => void
}

const STORAGE_COLUMN_KEYS = [
  'name',
  'provider',
  'location',
  'projects',
  'limit',
  'studioDefault',
  'createdBy',
] as const
type StorageColumnKey = typeof STORAGE_COLUMN_KEYS[number]

const columnLabelKeys = {
  name: 'table.name',
  provider: 'table.provider',
  location: 'table.location',
  projects: 'table.projects',
  limit: 'table.limit',
  studioDefault: 'table.studioDefault',
  createdBy: 'common:table.createdBy',
} as const satisfies Record<StorageColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  name: 200,
  provider: 180,
  location: 240,
  projects: 220,
  limit: 120,
  studioDefault: 140,
  createdBy: 160,
} as const satisfies Record<StorageColumnKey, number>

export function StorageLocationTable(props: Props) {
  const { t } = useTranslation([ 'storage', 'common' ])
  const labels = useSmartTableLabels()
  const userNames = useUserNames()
  const [ search, setSearch ] = useState('')
  useProjectsLoader()
  const projectNamesById = useAppSelector(selectProjectNamesById, shallowEqual)

  const managedColumns = useMemo(() => {
    // A Bunny zone's region by name, an AWS bucket's by code; Google Cloud needs none.
    function regionText(location: StorageLocation) {
      if (location.provider.kind === 'bunny') {
        return t(bunnyRegionLabelKeys[location.provider.region])
      }
      return location.provider.region ?? ''
    }

    // Deleted projects leave no name behind, so they are skipped.
    function projectsText(location: StorageLocation) {
      if (location.projects === ALL_PROJECTS) {
        return t('table.allProjects')
      }
      const names = location.projects
        .map((projectId) => projectNamesById[projectId])
        .filter(Boolean)
      if (!names.length) {
        return t('table.noProjects')
      }
      return names.join(', ')
    }

    const renderCell = {
      // Editing is the name's link, so it opens in a new tab too.
      name: (location: StorageLocation) => <Link
        href={getStorageLocationEditUrl(location.id)}
        className='font-medium text-link no-underline hover:underline'
      >{
          location.name
        }</Link>,
      provider: (location: StorageLocation) => <div>
        <div>{t(storageOptionLabelKeys[getStorageOption(location.provider)])}</div>
        {regionText(location) && <div className='text-xs opacity-70'>{regionText(location)}</div>}
      </div>,
      location: (location: StorageLocation) => <code className='text-xs break-all'>{getStoragePath(location)}</code>,
      projects: projectsText,
      limit: (location: StorageLocation) => location.storageLimitBytes === null
        ? t('table.noLimit')
        : formatBytes(location.storageLimitBytes),
      // Chosen on the location's edit page, behind its Save.
      studioDefault: (location: StorageLocation) => location.isStudioDefault && <Chip
        size='sm'
        variant='soft'
        color='accent'
      >{
          t('table.isStudioDefault')
        }</Chip>,
      createdBy: (location: StorageLocation) => userNames[location.createdBy] ?? t('common:table.someone'),
    } satisfies Record<StorageColumnKey, (location: StorageLocation) => unknown>

    // Search matches what the user sees.
    const searchValue = {
      name: (location: StorageLocation) => location.name,
      provider: (location: StorageLocation) => [
        t(storageOptionLabelKeys[getStorageOption(location.provider)]),
        regionText(location),
      ].join(' '),
      location: getStoragePath,
      projects: projectsText,
      limit: (location: StorageLocation) => location.storageLimitBytes === null
        ? t('table.noLimit')
        : formatBytes(location.storageLimitBytes),
      // Searching for "Studio default" finds the one that is.
      studioDefault: (location: StorageLocation) => location.isStudioDefault
        ? t('table.studioDefault')
        : '',
      createdBy: (location: StorageLocation) => userNames[location.createdBy] ?? '',
    } satisfies Record<StorageColumnKey, (location: StorageLocation) => string>

    // Sorting follows the text, except the limit, which sorts by size with no limit as the
    // largest, and the Studio default, which sorts first.
    const sortValue = {
      ...searchValue,
      limit: (location: StorageLocation) => location.storageLimitBytes ?? Number.POSITIVE_INFINITY,
      studioDefault: (location: StorageLocation) => location.isStudioDefault
        ? 0
        : 1,
    } satisfies Record<StorageColumnKey, (location: StorageLocation) => string | number>

    return createManagedColumns<StorageLocation, StorageColumnKey>({
      columnKeys: STORAGE_COLUMN_KEYS,
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
  }, [ t, userNames, projectNamesById ])

  const rowActions = useMemo((): DataTableAction<StorageLocation>[] => [
    {
      id: 'test',
      label: t('actions.test'),
      icon: <LuPlugZap className='size-4' aria-hidden />,
      onPress: (row) => props.onTest(row.original),
    },
    {
      id: 'delete',
      label: t('common:actions.delete'),
      icon: <LuTrash2 className='size-4' aria-hidden />,
      isDanger: true,
      onPress: (row) => props.onDelete(row.original),
    },
  ], [ t, props.onTest, props.onDelete ])

  if (!props.locations.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('table.empty')
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: 'storage-locations-table',
      tableLocalStorageId: 'elysium.settings.storage.table.v3',
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
    rowActions={rowActions}
    pagination={{ show: true }}
    enableSorting
    stickyHeader={false}
  />
}
