// Copyright © 2026 Jalapeno Labs

import type { DataTableAction } from '@jalapenolabs/uikit'
import type { AgentModel, Effort, ModelChoice } from '../../../api/routes/modelSettingRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuRotateCcw } from 'react-icons/lu'
import { OptionSelect } from '../../../components/OptionSelect'
import { ModelField } from './ModelField'

// Misc
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import { effortLabelKeys, kindLabelKeys } from './modelSettingsPresentation'

// A step or agent as a row: what the API holds, its words, and what the page shows now, its
// unsaved choice over what it runs on. `shown` is `null` for a step that runs no model.
export type ModelSettingRow = {
  agent: AgentModel
  name: string
  shown: ModelChoice | null
}

type Props = {
  rows: ModelSettingRow[]
  suggestedModels: string[]
  efforts: Effort[]
  onChange: (agent: AgentModel, choice: ModelChoice) => void
  onUseDefault: (agent: AgentModel) => Promise<void>
}

const MODEL_COLUMN_KEYS = [ 'name', 'kind', 'model', 'effort', 'setting' ] as const
type ModelColumnKey = typeof MODEL_COLUMN_KEYS[number]

const columnLabelKeys = {
  name: 'table.name',
  kind: 'table.kind',
  model: 'table.model',
  effort: 'table.effort',
  setting: 'table.setting',
} as const satisfies Record<ModelColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  name: 260,
  kind: 140,
  model: 260,
  effort: 160,
  setting: 120,
} as const satisfies Record<ModelColumnKey, number>

// Every step and agent, in the order the API lists them: each workflow's steps first to last,
// then the agents of their own. A step that runs no model says so instead of offering a
// choice. Model and effort are the page's fields, kept by its Save.
export function ModelSettingsTable(props: Props) {
  const { t } = useTranslation([ 'models', 'common' ])
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    const effortOptions = props.efforts.map((effort) => ({ id: effort, label: t(effortLabelKeys[effort]) }))

    function settingText(row: ModelSettingRow) {
      if (!row.agent.default) {
        return ''
      }
      return row.agent.setting
        ? t('table.chosen')
        : t('table.default')
    }

    const renderCell = {
      name: (row: ModelSettingRow) => <div className='flex flex-col'>
        <span className='font-medium'>{row.name}</span>
        <code className='font-mono text-xs opacity-60'>{row.agent.name}</code>
      </div>,
      kind: (row: ModelSettingRow) => <span className='opacity-80'>{t(kindLabelKeys[row.agent.kind])}</span>,
      model: (row: ModelSettingRow) => row.shown
        ? <ModelField
          id={`model-setting-${row.agent.name}-model`}
          label={t('table.modelFor', { name: row.name })}
          value={row.shown.model}
          suggestions={props.suggestedModels}
          invalidMessage={t('invalidModel')}
          onChange={(model) => {
            if (!row.shown) {
              return
            }
            props.onChange(row.agent, { model, effort: row.shown.effort })
          }}
        />
        : <span className='opacity-60'>{t('table.runsNoModel')}</span>,
      effort: (row: ModelSettingRow) => row.shown && <OptionSelect
        isLabelHidden
        id={`model-setting-${row.agent.name}-effort`}
        label={t('table.effortFor', { name: row.name })}
        options={effortOptions}
        value={row.shown.effort}
        onChange={(value) => {
          const effort = props.efforts.find((option) => option === value)
          if (!row.shown || !effort) {
            console.debug('ModelSettingsTable ignored an unknown effort', { value })
            return
          }
          props.onChange(row.agent, { model: row.shown.model, effort })
        }}
      />,
      setting: (row: ModelSettingRow) => row.agent.default && (row.agent.setting
        ? <Chip size='sm' variant='soft' color='accent'>{t('table.chosen')}</Chip>
        : <span className='opacity-60'>{t('table.default')}</span>),
    } satisfies Record<ModelColumnKey, (row: ModelSettingRow) => unknown>

    // Search matches what the admin sees.
    const searchValue = {
      name: (row: ModelSettingRow) => `${row.name} ${row.agent.name}`,
      kind: (row: ModelSettingRow) => t(kindLabelKeys[row.agent.kind]),
      model: (row: ModelSettingRow) => row.shown?.model ?? t('table.runsNoModel'),
      effort: (row: ModelSettingRow) => row.shown
        ? t(effortLabelKeys[row.shown.effort])
        : '',
      setting: settingText,
    } satisfies Record<ModelColumnKey, (row: ModelSettingRow) => string>

    return createManagedColumns<ModelSettingRow, ModelColumnKey>({
      columnKeys: MODEL_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: (columnKey) => columnKey,
      getSearchValue: (columnKey) => searchValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => ({
        id: columnId,
        header: columnLabel,
        accessorFn: searchValue[columnKey],
        // The order is the workflows' own, so the table does not sort.
        enableSorting: false,
        size: columnSizes[columnKey],
        cell: ({ row }) => renderCell[columnKey](row.original),
      }),
    })
  }, [ t, props.efforts, props.suggestedModels, props.onChange ])

  const rowActions = useMemo((): DataTableAction<ModelSettingRow>[] => [
    {
      id: 'useDefault',
      label: t('actions.useDefault'),
      icon: <LuRotateCcw className='size-4' aria-hidden />,
      isVisible: (row) => Boolean(row.original.agent.setting),
      onPress: (row) => props.onUseDefault(row.original.agent),
    },
  ], [ t, props.onUseDefault ])

  return <SmartTable
    ids={{
      tableElementId: 'model-settings-table',
      tableLocalStorageId: 'elysium.settings.models.table.v1',
    }}
    tableAriaLabel={t('table.label')}
    data={props.rows}
    managedColumns={managedColumns}
    getRowId={(row) => row.agent.name}
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
