// Copyright © 2026 Jalapeno Labs

import type { EnvironmentVariable } from '../../../api/routes/environmentRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { EnvironmentVariableRowActions } from './EnvironmentVariableRowActions'

// Misc
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import { MASKED_VALUE } from './environmentPresentation'

type Props = {
  variables: EnvironmentVariable[]
  onEdit: (variable: EnvironmentVariable) => void
  onDelete: (variable: EnvironmentVariable) => void
}

const ENVIRONMENT_COLUMN_KEYS = [ 'key', 'value', 'description', 'rowActions' ] as const
type EnvironmentColumnKey = typeof ENVIRONMENT_COLUMN_KEYS[number]

const columnLabelKeys = {
  key: 'table.key',
  value: 'table.value',
  description: 'table.description',
  rowActions: 'common:actions.moreActions',
} as const satisfies Record<EnvironmentColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  key: 240,
  value: 280,
  description: 320,
  rowActions: 64,
} as const satisfies Record<EnvironmentColumnKey, number>

export function EnvironmentVariableTable(props: Props) {
  const { t } = useTranslation([ 'environment', 'common' ])
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    const renderCell = {
      key: (variable: EnvironmentVariable) => <code className='font-mono text-xs font-medium'>{
        variable.key
      }</code>,
      value: (variable: EnvironmentVariable) => {
        if (variable.isSecret) {
          return <div className='flex items-center gap-2'>
            <code className='font-mono text-xs opacity-70'>{MASKED_VALUE}</code>
            <Chip size='sm' variant='soft'>{t('table.secret')}</Chip>
          </div>
        }
        if (!variable.value) {
          return <span className='text-xs opacity-50'>{t('table.emptyValue')}</span>
        }
        return <code className='block truncate font-mono text-xs' title={variable.value}>{
          variable.value
        }</code>
      },
      description: (variable: EnvironmentVariable) => <span className='line-clamp-2 opacity-80'>{
        variable.description
      }</span>,
      rowActions: (variable: EnvironmentVariable) => <EnvironmentVariableRowActions
        variable={variable}
        onEdit={props.onEdit}
        onDelete={props.onDelete}
      />,
    } satisfies Record<EnvironmentColumnKey, (variable: EnvironmentVariable) => unknown>

    // Search matches what the user sees: a secret's value is only ever the word Secret.
    const searchValue = {
      key: (variable: EnvironmentVariable) => variable.key,
      value: (variable: EnvironmentVariable) => variable.isSecret
        ? t('table.secret')
        : variable.value ?? '',
      description: (variable: EnvironmentVariable) => variable.description,
      rowActions: null,
    } satisfies Record<EnvironmentColumnKey, ((variable: EnvironmentVariable) => string) | null>

    return createManagedColumns<EnvironmentVariable, EnvironmentColumnKey>({
      columnKeys: ENVIRONMENT_COLUMN_KEYS,
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
  }, [ t, props.onEdit, props.onDelete ])

  if (!props.variables.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('table.empty')
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: 'environment-variables-table',
      tableLocalStorageId: 'elysium.settings.environment.table',
    }}
    tableAriaLabel={t('table.label')}
    data={props.variables}
    managedColumns={managedColumns}
    getRowId={(variable) => variable.id}
    labels={labels}
    search={{
      value: search,
      onChange: setSearch,
    }}
    enableSorting
    stickyHeader={false}
  />
}
