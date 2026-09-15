// Copyright © 2026 Jalapeno Labs

import type { Llm } from '../../../api/routes/llmRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LlmRowActions } from './LlmRowActions'

// Misc
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import {
  getLlmStatus,
  llmStatusChipColors,
  llmStatusLabelKeys,
  llmTypeLabelKeys,
} from './llmPresentation'

type Props = {
  llms: Llm[]
  onEdit: (llm: Llm) => void
  onToggleActive: (llm: Llm) => void
  onDelete: (llm: Llm) => void
}

const LLM_COLUMN_KEYS = [ 'name', 'type', 'priority', 'status', 'expiresAt', 'rowActions' ] as const
type LlmColumnKey = typeof LLM_COLUMN_KEYS[number]

const columnLabelKeys = {
  name: 'table.name',
  type: 'table.type',
  priority: 'table.priority',
  status: 'table.status',
  expiresAt: 'table.expires',
  rowActions: 'common:actions.moreActions',
} as const satisfies Record<LlmColumnKey, string>

// Starting widths in pixels. They sum to less than the settings content column,
// so the actions column stays in view without horizontal scrolling.
const columnSizes = {
  name: 280,
  type: 200,
  priority: 110,
  status: 140,
  expiresAt: 220,
  rowActions: 64,
} as const satisfies Record<LlmColumnKey, number>

export function LlmTable(props: Props) {
  const { t, i18n } = useTranslation([ 'llms', 'common' ])
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  // Captured once per mount so render stays pure; status is "as of when you opened the page".
  const [ now ] = useState(() => Date.now())

  const managedColumns = useMemo(() => {
    // Timestamps arrive as UTC; this is the one place they become local time.
    // Expiry is chosen as a date, so the time of day would only be noise.
    const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium' })

    const renderCell = {
      name: (llm: Llm) => <div>
        <div className='font-medium'>{llm.name}</div>
        {llm.description && <div className='max-w-xs truncate text-xs opacity-70'>{
          llm.description
        }</div>}
      </div>,
      type: (llm: Llm) => t(llmTypeLabelKeys[llm.type]),
      priority: (llm: Llm) => llm.priority,
      status: (llm: Llm) => {
        const status = getLlmStatus(llm, now)
        return <Chip size='sm' variant='soft' color={llmStatusChipColors[status]}>{
          t(llmStatusLabelKeys[status])
        }</Chip>
      },
      expiresAt: (llm: Llm) => llm.expiresAt
        ? dateFormatter.format(new Date(llm.expiresAt))
        : t('table.never'),
      rowActions: (llm: Llm) => <LlmRowActions
        llm={llm}
        onEdit={props.onEdit}
        onToggleActive={props.onToggleActive}
        onDelete={props.onDelete}
      />,
    } satisfies Record<LlmColumnKey, (llm: Llm) => unknown>

    // Search matches what the user sees, not raw enum values or timestamps.
    const searchValue = {
      name: (llm: Llm) => `${llm.name} ${llm.description}`,
      type: (llm: Llm) => t(llmTypeLabelKeys[llm.type]),
      priority: (llm: Llm) => String(llm.priority),
      status: (llm: Llm) => t(llmStatusLabelKeys[getLlmStatus(llm, now)]),
      expiresAt: (llm: Llm) => llm.expiresAt
        ? dateFormatter.format(new Date(llm.expiresAt))
        : t('table.never'),
      rowActions: null,
    } satisfies Record<LlmColumnKey, ((llm: Llm) => string) | null>

    return createManagedColumns<Llm, LlmColumnKey>({
      columnKeys: LLM_COLUMN_KEYS,
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
  }, [ t, i18n.language, now, props.onEdit, props.onToggleActive, props.onDelete ])

  if (!props.llms.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('table.empty')
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: 'llm-credentials-table',
      tableLocalStorageId: 'elysium.settings.llms.table',
    }}
    tableAriaLabel={t('table.label')}
    data={props.llms}
    managedColumns={managedColumns}
    getRowId={(llm) => llm.id}
    labels={labels}
    search={{
      value: search,
      onChange: setSearch,
    }}
    enableSorting
    stickyHeader={false}
  />
}
