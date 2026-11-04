// Copyright © 2026 Jalapeno Labs

import type { DataTableAction } from '@jalapenolabs/uikit'
import type { KindModel, ModelChoice, ModelChoices } from '../../../api/routes/modelSettingRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuRotateCcw } from 'react-icons/lu'
import { EffortField } from './EffortField'
import { ModelField } from './ModelField'
import { ModelPriceText } from './ModelPriceText'
import { RecordedUsageText } from './RecordedUsageText'

// Misc
import { formatShortDate } from '../../../format/dates'
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import { effortLabelKeys } from './modelSettingsPresentation'

// A kind as a row: what the API holds, its words, the steps and agents it covers in words (or
// `null` for none yet), and what the page shows now, its unsaved choice over what it runs on.
export type KindModelRow = {
  kind: KindModel
  name: string
  members: string | null
  shown: ModelChoice
}

type Props = {
  rows: KindModelRow[]
  choices: ModelChoices
  // Where each kind's spend starts counting.
  usageSince: string
  onChange: (kind: KindModel, choice: ModelChoice) => void
  onUseDefault: (kind: KindModel) => Promise<void>
}

const KIND_COLUMN_KEYS = [ 'name', 'model', 'effort', 'setting', 'price', 'spent' ] as const
type KindColumnKey = typeof KIND_COLUMN_KEYS[number]

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  name: 240,
  model: 240,
  effort: 150,
  setting: 110,
  price: 130,
  spent: 130,
} as const satisfies Record<KindColumnKey, number>

// Every kind of agent, in the order the API lists them, each with what its agents run on, its
// price, and what its turns spent. Model and effort are the page's fields, kept by its Save.
export function KindModelsTable(props: Props) {
  const { t } = useTranslation([ 'models', 'common' ])
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    const columnLabels = {
      name: t('kindsTable.name'),
      model: t('table.model'),
      effort: t('table.effort'),
      setting: t('table.setting'),
      price: t('kindsTable.price'),
      spent: t('kindsTable.spent', { date: formatShortDate(props.usageSince) }),
    } satisfies Record<KindColumnKey, string>

    const renderCell = {
      name: (row: KindModelRow) => <div className='flex flex-col'>
        <span className='font-medium'>{row.name}</span>
        <span className='text-xs opacity-60'>{row.members ?? t('kindsTable.noMembers')}</span>
      </div>,
      model: (row: KindModelRow) => <ModelField
        id={`model-setting-kind-${row.kind.name}-model`}
        label={t('table.modelFor', { name: row.name })}
        value={row.shown.model}
        suggestions={props.choices.suggestedModels}
        invalidMessage={t('invalidModel')}
        onChange={(model) => props.onChange(row.kind, { model, effort: row.shown.effort })}
      />,
      effort: (row: KindModelRow) => <EffortField
        id={`model-setting-kind-${row.kind.name}-effort`}
        label={t('table.effortFor', { name: row.name })}
        efforts={props.choices.efforts}
        value={row.shown.effort}
        onChange={(effort) => props.onChange(row.kind, { model: row.shown.model, effort })}
      />,
      setting: (row: KindModelRow) => row.kind.setting
        ? <Chip size='sm' variant='soft' color='accent'>{t('table.chosen')}</Chip>
        : <span className='opacity-60'>{t('table.default')}</span>,
      price: (row: KindModelRow) => <ModelPriceText model={row.shown.model} prices={props.choices.modelPrices} />,
      spent: (row: KindModelRow) => <RecordedUsageText usage={row.kind.usage} />,
    } satisfies Record<KindColumnKey, (row: KindModelRow) => unknown>

    // Search matches what the admin sees.
    const searchValue = {
      name: (row: KindModelRow) => `${row.name} ${row.members ?? ''}`,
      model: (row: KindModelRow) => row.shown.model,
      effort: (row: KindModelRow) => t(effortLabelKeys[row.shown.effort]),
      setting: (row: KindModelRow) => row.kind.setting
        ? t('table.chosen')
        : t('table.default'),
      // Prices and spend are amounts to compare, not words to find.
      price: () => '',
      spent: () => '',
    } satisfies Record<KindColumnKey, (row: KindModelRow) => string>

    return createManagedColumns<KindModelRow, KindColumnKey>({
      columnKeys: KIND_COLUMN_KEYS,
      getColumnLabel: (columnKey) => columnLabels[columnKey],
      getSearchKey: (columnKey) => columnKey,
      getSearchValue: (columnKey) => searchValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => ({
        id: columnId,
        header: columnLabel,
        accessorFn: searchValue[columnKey],
        // The order is the API's own, so the table does not sort.
        enableSorting: false,
        size: columnSizes[columnKey],
        cell: ({ row }) => renderCell[columnKey](row.original),
      }),
    })
  }, [ t, props.choices, props.usageSince, props.onChange ])

  const rowActions = useMemo((): DataTableAction<KindModelRow>[] => [
    {
      id: 'useDefault',
      label: t('actions.useDefault'),
      icon: <LuRotateCcw className='size-4' aria-hidden />,
      isVisible: (row) => Boolean(row.original.kind.setting),
      onPress: (row) => props.onUseDefault(row.original.kind),
    },
  ], [ t, props.onUseDefault ])

  return <SmartTable
    ids={{
      tableElementId: 'kind-model-settings-table',
      tableLocalStorageId: 'elysium.settings.models.kinds.table.v1',
    }}
    tableAriaLabel={t('kindsTable.label')}
    data={props.rows}
    managedColumns={managedColumns}
    getRowId={(row) => row.kind.name}
    labels={labels}
    search={{
      value: search,
      onChange: setSearch,
    }}
    rowActions={rowActions}
    pagination={{ show: true }}
    stickyHeader={false}
  />
}
