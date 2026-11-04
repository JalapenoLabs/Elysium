// Copyright © 2026 Jalapeno Labs

import type { DataTableAction } from '@jalapenolabs/uikit'
import type {
  ModelChoice,
  ModelChoices,
  ProjectKindModel,
} from '../../api/routes/modelSettingRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuRotateCcw } from 'react-icons/lu'
import { EffortField } from '../Settings/Models/EffortField'
import { ModelField } from '../Settings/Models/ModelField'
import { ModelPriceText } from '../Settings/Models/ModelPriceText'
import { RecordedUsageText } from '../Settings/Models/RecordedUsageText'

// Misc
import { formatShortDate } from '../../format/dates'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import { effortLabelKeys } from '../Settings/Models/modelSettingsPresentation'

// A kind as a row: what the API holds, its words, and what the page shows now, its unsaved
// choice over what it runs on for the project.
export type ProjectKindModelRow = {
  kind: ProjectKindModel
  name: string
  shown: ModelChoice
}

type Props = {
  rows: ProjectKindModelRow[]
  choices: ModelChoices
  // Where each kind's spend starts counting.
  usageSince: string
  onChange: (kind: ProjectKindModel, choice: ModelChoice) => void
  onUseWorkspace: (kind: ProjectKindModel) => Promise<void>
}

const PROJECT_KIND_COLUMN_KEYS = [ 'name', 'model', 'effort', 'source', 'price', 'spent' ] as const
type ProjectKindColumnKey = typeof PROJECT_KIND_COLUMN_KEYS[number]

// Starting widths in pixels, summing to less than the page's content column.
const columnSizes = {
  name: 200,
  model: 260,
  effort: 150,
  source: 130,
  price: 130,
  spent: 140,
} as const satisfies Record<ProjectKindColumnKey, number>

// The kinds of agent that work for one project, each with what it runs on here, whether that is
// the project's own choice, its price, and what its turns for the project spent.
export function ProjectModelsTable(props: Props) {
  const { t } = useTranslation([ 'models', 'common' ])
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    const columnLabels = {
      name: t('kindsTable.name'),
      model: t('table.model'),
      effort: t('table.effort'),
      source: t('agentsTable.source'),
      price: t('kindsTable.price'),
      spent: t('kindsTable.spent', { date: formatShortDate(props.usageSince) }),
    } satisfies Record<ProjectKindColumnKey, string>

    function sourceText(row: ProjectKindModelRow) {
      return row.kind.setting
        ? t('sources.project')
        : t('project.following')
    }

    const renderCell = {
      name: (row: ProjectKindModelRow) => <span className='font-medium'>{row.name}</span>,
      model: (row: ProjectKindModelRow) => <ModelField
        id={`project-model-${row.kind.name}-model`}
        label={t('table.modelFor', { name: row.name })}
        value={row.shown.model}
        suggestions={props.choices.suggestedModels}
        invalidMessage={t('invalidModel')}
        onChange={(model) => props.onChange(row.kind, { model, effort: row.shown.effort })}
      />,
      effort: (row: ProjectKindModelRow) => <EffortField
        id={`project-model-${row.kind.name}-effort`}
        label={t('table.effortFor', { name: row.name })}
        efforts={props.choices.efforts}
        value={row.shown.effort}
        onChange={(effort) => props.onChange(row.kind, { model: row.shown.model, effort })}
      />,
      // The project's own choice stands out; following the workspace reads quietly.
      source: (row: ProjectKindModelRow) => row.kind.setting
        ? <Chip size='sm' variant='soft' color='accent'>{sourceText(row)}</Chip>
        : <span className='opacity-60'>{sourceText(row)}</span>,
      price: (row: ProjectKindModelRow) => <ModelPriceText
        model={row.shown.model}
        prices={props.choices.modelPrices}
      />,
      spent: (row: ProjectKindModelRow) => <RecordedUsageText usage={row.kind.usage} />,
    } satisfies Record<ProjectKindColumnKey, (row: ProjectKindModelRow) => unknown>

    // Search matches what the person sees.
    const searchValue = {
      name: (row: ProjectKindModelRow) => row.name,
      model: (row: ProjectKindModelRow) => row.shown.model,
      effort: (row: ProjectKindModelRow) => t(effortLabelKeys[row.shown.effort]),
      source: sourceText,
      // Prices and spend are amounts to compare, not words to find.
      price: () => '',
      spent: () => '',
    } satisfies Record<ProjectKindColumnKey, (row: ProjectKindModelRow) => string>

    return createManagedColumns<ProjectKindModelRow, ProjectKindColumnKey>({
      columnKeys: PROJECT_KIND_COLUMN_KEYS,
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

  const rowActions = useMemo((): DataTableAction<ProjectKindModelRow>[] => [
    {
      id: 'useWorkspace',
      label: t('project.useWorkspace'),
      icon: <LuRotateCcw className='size-4' aria-hidden />,
      isVisible: (row) => Boolean(row.original.kind.setting),
      onPress: (row) => props.onUseWorkspace(row.original.kind),
    },
  ], [ t, props.onUseWorkspace ])

  return <SmartTable
    ids={{
      tableElementId: 'project-model-settings-table',
      tableLocalStorageId: 'elysium.projects.models.table.v1',
    }}
    tableAriaLabel={t('project.tableLabel')}
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
