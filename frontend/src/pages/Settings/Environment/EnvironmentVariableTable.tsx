// Copyright © 2026 Jalapeno Labs

import type { DataTableAction } from '@jalapenolabs/uikit'
import type { EnvironmentVariable } from '../../../api/routes/environmentRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip, Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuTrash2 } from 'react-icons/lu'

// Misc
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import { useUserNames } from '../../../hooks/useUserNames'
import { getEnvironmentVariableEditUrl } from '../../../urls'

type Props = {
  variables: EnvironmentVariable[]
  onDelete: (variable: EnvironmentVariable) => void
}

const ENVIRONMENT_COLUMN_KEYS = [ 'key', 'value', 'description', 'createdBy' ] as const
type EnvironmentColumnKey = typeof ENVIRONMENT_COLUMN_KEYS[number]

const columnLabelKeys = {
  key: 'table.key',
  value: 'table.value',
  description: 'table.description',
  createdBy: 'common:table.createdBy',
} as const satisfies Record<EnvironmentColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  key: 240,
  value: 280,
  description: 320,
  createdBy: 160,
} as const satisfies Record<EnvironmentColumnKey, number>

export function EnvironmentVariableTable(props: Props) {
  const { t } = useTranslation([ 'environment', 'common' ])
  const labels = useSmartTableLabels()
  const userNames = useUserNames()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    const renderCell = {
      // Editing is the key's link, so it opens in a new tab too.
      key: (variable: EnvironmentVariable) => <Link
        href={getEnvironmentVariableEditUrl(variable.id)}
        className='text-link no-underline hover:underline'
      >
        <code className='font-mono text-xs font-medium break-all'>{variable.key}</code>
      </Link>,
      // The API never returns a secret's value, so a secret shows only that it is one.
      value: (variable: EnvironmentVariable) => {
        if (variable.isSecret) {
          return <Chip size='sm' variant='soft'>{t('table.secret')}</Chip>
        }
        if (!variable.value) {
          return <span className='text-xs opacity-50'>{t('table.emptyValue')}</span>
        }
        return <code className='font-mono text-xs break-all'>{variable.value}</code>
      },
      description: (variable: EnvironmentVariable) => <span className='opacity-80'>{
        variable.description
      }</span>,
      createdBy: (variable: EnvironmentVariable) => userNames[variable.createdBy] ?? t('common:table.someone'),
    } satisfies Record<EnvironmentColumnKey, (variable: EnvironmentVariable) => unknown>

    // Search and sorting follow what the user sees: a secret's value is only ever the word Secret.
    const searchValue = {
      key: (variable: EnvironmentVariable) => variable.key,
      value: (variable: EnvironmentVariable) => variable.isSecret
        ? t('table.secret')
        : variable.value ?? '',
      description: (variable: EnvironmentVariable) => variable.description,
      createdBy: (variable: EnvironmentVariable) => userNames[variable.createdBy] ?? '',
    } satisfies Record<EnvironmentColumnKey, (variable: EnvironmentVariable) => string>

    return createManagedColumns<EnvironmentVariable, EnvironmentColumnKey>({
      columnKeys: ENVIRONMENT_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: (columnKey) => columnKey,
      getSearchValue: (columnKey) => searchValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => ({
        id: columnId,
        header: columnLabel,
        accessorFn: searchValue[columnKey],
        size: columnSizes[columnKey],
        cell: ({ row }) => renderCell[columnKey](row.original),
      }),
    })
  }, [ t, userNames ])

  const rowActions = useMemo((): DataTableAction<EnvironmentVariable>[] => [
    {
      id: 'delete',
      label: t('common:actions.delete'),
      icon: <LuTrash2 className='size-4' aria-hidden />,
      isDanger: true,
      onPress: (row) => props.onDelete(row.original),
    },
  ], [ t, props.onDelete ])

  if (!props.variables.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('table.empty')
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: 'environment-variables-table',
      tableLocalStorageId: 'elysium.settings.environment.table.v2',
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
    rowActions={rowActions}
    pagination={{ show: true }}
    enableSorting
    stickyHeader={false}
  />
}
