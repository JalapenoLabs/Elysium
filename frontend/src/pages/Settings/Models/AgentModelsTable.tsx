// Copyright © 2026 Jalapeno Labs

import type { DataTableAction } from '@jalapenolabs/uikit'
import type { AgentModel, ModelChoice, ModelChoices } from '../../../api/routes/modelSettingRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuRotateCcw } from 'react-icons/lu'
import { EffortField } from './EffortField'
import { ModelField } from './ModelField'

// Misc
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import { effortLabelKeys, kindLabelKeys, sourceLabelKeys } from './modelSettingsPresentation'

// A step or agent as a row: what the API holds, its words, and what the page shows now, its
// unsaved choice over what it runs on. `shown` is `null` for a step that runs no model.
export type AgentModelRow = {
  agent: AgentModel
  name: string
  shown: ModelChoice | null
}

type Props = {
  rows: AgentModelRow[]
  choices: ModelChoices
  onChange: (agent: AgentModel, choice: ModelChoice) => void
  onFollowKind: (agent: AgentModel) => Promise<void>
}

const AGENT_COLUMN_KEYS = [ 'name', 'kind', 'model', 'effort', 'source' ] as const
type AgentColumnKey = typeof AGENT_COLUMN_KEYS[number]

const columnLabelKeys = {
  name: 'agentsTable.name',
  kind: 'agentsTable.kind',
  model: 'table.model',
  effort: 'table.effort',
  source: 'agentsTable.source',
} as const satisfies Record<AgentColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  name: 260,
  kind: 150,
  model: 260,
  effort: 150,
  source: 140,
} as const satisfies Record<AgentColumnKey, number>

// Every step and agent, in the order the API lists them: each workflow's steps first to last,
// then the agents of their own, each with its kind and why it runs on what it does. A step that
// runs no model says so instead of offering a choice. Model and effort are the page's fields,
// kept by its Save.
export function AgentModelsTable(props: Props) {
  const { t } = useTranslation([ 'models', 'common' ])
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    function kindText(row: AgentModelRow) {
      return row.agent.kind
        ? t(kindLabelKeys[row.agent.kind])
        : ''
    }

    function sourceText(row: AgentModelRow) {
      return row.agent.resolved
        ? t(sourceLabelKeys[row.agent.resolved.source])
        : ''
    }

    const renderCell = {
      name: (row: AgentModelRow) => <div className='flex flex-col'>
        <span className='font-medium'>{row.name}</span>
        <code className='font-mono text-xs opacity-60'>{row.agent.name}</code>
      </div>,
      kind: (row: AgentModelRow) => <span className='opacity-80'>{kindText(row)}</span>,
      model: (row: AgentModelRow) => row.shown
        ? <ModelField
          id={`model-setting-${row.agent.name}-model`}
          label={t('table.modelFor', { name: row.name })}
          value={row.shown.model}
          suggestions={props.choices.suggestedModels}
          invalidMessage={t('invalidModel')}
          onChange={(model) => {
            if (!row.shown) {
              return
            }
            props.onChange(row.agent, { model, effort: row.shown.effort })
          }}
        />
        : <span className='opacity-60'>{t('table.runsNoModel')}</span>,
      effort: (row: AgentModelRow) => row.shown && <EffortField
        id={`model-setting-${row.agent.name}-effort`}
        label={t('table.effortFor', { name: row.name })}
        efforts={props.choices.efforts}
        value={row.shown.effort}
        onChange={(effort) => {
          if (!row.shown) {
            return
          }
          props.onChange(row.agent, { model: row.shown.model, effort })
        }}
      />,
      // Its own setting stands out; following its kind or the default reads quietly.
      source: (row: AgentModelRow) => row.agent.resolved?.source === 'agent'
        ? <Chip size='sm' variant='soft' color='accent'>{sourceText(row)}</Chip>
        : <span className='opacity-60'>{sourceText(row)}</span>,
    } satisfies Record<AgentColumnKey, (row: AgentModelRow) => unknown>

    // Search matches what the admin sees.
    const searchValue = {
      name: (row: AgentModelRow) => `${row.name} ${row.agent.name}`,
      kind: kindText,
      model: (row: AgentModelRow) => row.shown?.model ?? t('table.runsNoModel'),
      effort: (row: AgentModelRow) => row.shown
        ? t(effortLabelKeys[row.shown.effort])
        : '',
      source: sourceText,
    } satisfies Record<AgentColumnKey, (row: AgentModelRow) => string>

    return createManagedColumns<AgentModelRow, AgentColumnKey>({
      columnKeys: AGENT_COLUMN_KEYS,
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
  }, [ t, props.choices, props.onChange ])

  const rowActions = useMemo((): DataTableAction<AgentModelRow>[] => [
    {
      id: 'followKind',
      label: t('actions.followKind'),
      icon: <LuRotateCcw className='size-4' aria-hidden />,
      isVisible: (row) => Boolean(row.original.agent.setting),
      onPress: (row) => props.onFollowKind(row.original.agent),
    },
  ], [ t, props.onFollowKind ])

  return <SmartTable
    ids={{
      tableElementId: 'model-settings-table',
      tableLocalStorageId: 'elysium.settings.models.table.v2',
    }}
    tableAriaLabel={t('agentsTable.label')}
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
