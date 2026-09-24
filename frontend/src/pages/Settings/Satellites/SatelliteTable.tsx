// Copyright © 2026 Jalapeno Labs

import type { Satellite } from '../../../api/routes/satelliteRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip, Tooltip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { SatelliteRowActions } from './SatelliteRowActions'

// Misc
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import {
  getSatelliteHealth,
  getSatelliteSetupDisplay,
  satelliteHealthChipColors,
  satelliteHealthLabelKeys,
  satelliteSetupChipColors,
  satelliteSetupLabelKeys,
} from './satellitePresentation'

type Props = {
  satellites: Satellite[]
  onEdit: (satellite: Satellite) => void
  onTest: (satellite: Satellite) => void
  onToggleActive: (satellite: Satellite) => void
  onDelete: (satellite: Satellite) => void
}

const SATELLITE_COLUMN_KEYS = [ 'name', 'url', 'status', 'version', 'threads', 'setup', 'rowActions' ] as const
type SatelliteColumnKey = typeof SATELLITE_COLUMN_KEYS[number]

const columnLabelKeys = {
  name: 'table.name',
  url: 'table.url',
  status: 'table.status',
  version: 'table.version',
  threads: 'table.threads',
  setup: 'table.setup',
  rowActions: 'common:actions.moreActions',
} as const satisfies Record<SatelliteColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  name: 240,
  url: 260,
  status: 150,
  version: 120,
  threads: 170,
  setup: 130,
  rowActions: 64,
} as const satisfies Record<SatelliteColumnKey, number>

export function SatelliteTable(props: Props) {
  const { t } = useTranslation([ 'satellites', 'common' ])
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    function threadsText(satellite: Satellite) {
      const status = satellite.status
      if (!satellite.isActive || status?.runningThreads == null || status.maxConcurrentThreads == null) {
        return t('table.unknown')
      }
      return t('table.threadsValue', { running: status.runningThreads, max: status.maxConcurrentThreads })
    }

    const renderCell = {
      name: (satellite: Satellite) => <div>
        <div className='font-medium'>{satellite.name}</div>
        {satellite.description && <div className='max-w-xs truncate text-xs opacity-70'>{
          satellite.description
        }</div>}
      </div>,
      url: (satellite: Satellite) => <code className='text-xs'>{satellite.url}</code>,
      status: (satellite: Satellite) => {
        const health = getSatelliteHealth(satellite)
        const chip = <Chip size='sm' variant='soft' color={satelliteHealthChipColors[health]}>{
          t(satelliteHealthLabelKeys[health])
        }</Chip>

        // An unreachable satellite explains itself on hover.
        if (health !== 'offline' || !satellite.status?.error) {
          return chip
        }
        return <Tooltip delay={200}>
          <Tooltip.Trigger>{chip}</Tooltip.Trigger>
          <Tooltip.Content className='max-w-sm'>
            <span>{satellite.status.error}</span>
          </Tooltip.Content>
        </Tooltip>
      },
      version: (satellite: Satellite) => satellite.status?.version ?? t('table.unknown'),
      threads: threadsText,
      setup: (satellite: Satellite) => {
        const display = getSatelliteSetupDisplay(satellite)
        if (!display) {
          return t('table.unknown')
        }
        const chip = <Chip size='sm' variant='soft' color={satelliteSetupChipColors[display]}>{
          t(satelliteSetupLabelKeys[display])
        }</Chip>

        // A failed install explains itself on hover with the end of its output.
        const failureOutput = satellite.status?.setup?.failureOutput
        if (display !== 'failed' || !failureOutput) {
          return chip
        }
        return <Tooltip delay={200}>
          <Tooltip.Trigger>{chip}</Tooltip.Trigger>
          <Tooltip.Content className='max-w-lg'>
            <p className='compact'>{t('setup.failedHint')}</p>
            <pre className='max-h-64 overflow-auto whitespace-pre-wrap text-xs'>{failureOutput}</pre>
          </Tooltip.Content>
        </Tooltip>
      },
      rowActions: (satellite: Satellite) => <SatelliteRowActions
        satellite={satellite}
        onEdit={props.onEdit}
        onTest={props.onTest}
        onToggleActive={props.onToggleActive}
        onDelete={props.onDelete}
      />,
    } satisfies Record<SatelliteColumnKey, (satellite: Satellite) => unknown>

    // Search matches what the user sees.
    const searchValue = {
      name: (satellite: Satellite) => `${satellite.name} ${satellite.description}`,
      url: (satellite: Satellite) => satellite.url,
      status: (satellite: Satellite) => t(satelliteHealthLabelKeys[getSatelliteHealth(satellite)]),
      version: (satellite: Satellite) => satellite.status?.version ?? '',
      threads: threadsText,
      setup: (satellite: Satellite) => {
        const display = getSatelliteSetupDisplay(satellite)
        return display
          ? t(satelliteSetupLabelKeys[display])
          : ''
      },
      rowActions: null,
    } satisfies Record<SatelliteColumnKey, ((satellite: Satellite) => string) | null>

    return createManagedColumns<Satellite, SatelliteColumnKey>({
      columnKeys: SATELLITE_COLUMN_KEYS,
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

        return {
          id: columnId,
          header: columnLabel,
          accessorFn: toSearchText,
          size: columnSizes[columnKey],
          cell: ({ row }) => renderCell[columnKey](row.original),
        }
      },
    })
  }, [ t, props.onEdit, props.onTest, props.onToggleActive, props.onDelete ])

  if (!props.satellites.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('table.empty')
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: 'satellites-table',
      tableLocalStorageId: 'elysium.settings.satellites.table',
    }}
    tableAriaLabel={t('table.label')}
    data={props.satellites}
    managedColumns={managedColumns}
    getRowId={(satellite) => satellite.id}
    labels={labels}
    search={{
      value: search,
      onChange: setSearch,
    }}
    enableSorting
    stickyHeader={false}
  />
}
