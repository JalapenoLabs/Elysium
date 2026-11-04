// Copyright © 2026 Jalapeno Labs

import type { LearningCategory } from '../../../api/routes/learningCategoryRoutes'
import type { PresentedLearningCategory } from '../Learnings/learningPresentation'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LearningCategoryRowActions } from './LearningCategoryRowActions'

// Misc
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'

// A category as a row: as people read it, with what the API holds of it.
export type LearningCategoryRow = {
  presented: PresentedLearningCategory
  stored: LearningCategory
}

type Props = {
  // In the order they are offered: built-in ones first, then the workspace's own.
  rows: LearningCategoryRow[]
  onMove: (category: LearningCategory, direction: 'up' | 'down') => void
  onRemove: (category: LearningCategory) => void
}

const CATEGORY_COLUMN_KEYS = [ 'category', 'description', 'learnings', 'rowActions' ] as const
type CategoryColumnKey = typeof CATEGORY_COLUMN_KEYS[number]

const columnLabelKeys = {
  category: 'table.category',
  description: 'table.description',
  learnings: 'table.learnings',
  rowActions: 'common:actions.moreActions',
} as const satisfies Record<CategoryColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  category: 220,
  description: 460,
  learnings: 110,
  rowActions: 96,
} as const satisfies Record<CategoryColumnKey, number>

// Every category in the order agents and people are offered them. The order is the point, so
// the table does not sort; the workspace's own move with their row menu. A built-in row says so
// where the menu would be.
export function LearningCategoryTable(props: Props) {
  const { t } = useTranslation([ 'learningCategories', 'common' ])
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    // The workspace's own, in order, which is what moving up and down moves among.
    const customKeys: string[] = []
    for (const row of props.rows) {
      if (!row.stored.builtIn) {
        customKeys.push(row.stored.key)
      }
    }

    const renderCell = {
      category: (row: LearningCategoryRow) => <div className='flex flex-col'>
        <span className='font-medium'>{row.presented.label}</span>
        <code className='font-mono text-xs opacity-60'>{row.stored.key}</code>
      </div>,
      description: (row: LearningCategoryRow) => <span className='line-clamp-3 opacity-80'>{
        row.presented.description
      }</span>,
      learnings: (row: LearningCategoryRow) => <span className='opacity-80'>{
        row.stored.learningCount
      }</span>,
      rowActions: (row: LearningCategoryRow) => {
        if (row.stored.builtIn) {
          return <Chip size='sm' variant='soft'>{t('table.builtIn')}</Chip>
        }
        const place = customKeys.indexOf(row.stored.key)
        return <LearningCategoryRowActions
          category={row.stored}
          canMoveUp={place > 0}
          canMoveDown={place < customKeys.length - 1}
          onMove={props.onMove}
          onRemove={props.onRemove}
        />
      },
    } satisfies Record<CategoryColumnKey, (row: LearningCategoryRow) => unknown>

    const searchValue = {
      category: (row: LearningCategoryRow) => `${row.presented.label} ${row.stored.key}`,
      description: (row: LearningCategoryRow) => row.presented.description,
      learnings: null,
      rowActions: null,
    } satisfies Record<CategoryColumnKey, ((row: LearningCategoryRow) => string) | null>

    return createManagedColumns<LearningCategoryRow, CategoryColumnKey>({
      columnKeys: CATEGORY_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: (columnKey) => searchValue[columnKey]
        ? columnKey
        : null,
      getSearchValue: (columnKey) => searchValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => ({
        id: columnId,
        header: columnKey === 'rowActions'
          ? () => <span className='sr-only'>{columnLabel}</span>
          : columnLabel,
        accessorFn: searchValue[columnKey] ?? undefined,
        enableSorting: false,
        size: columnSizes[columnKey],
        cell: ({ row }) => renderCell[columnKey](row.original),
      }),
    })
  }, [ t, props.rows, props.onMove, props.onRemove ])

  return <SmartTable
    ids={{
      tableElementId: 'learning-categories-table',
      tableLocalStorageId: 'elysium.settings.learningCategories.table.v1',
    }}
    tableAriaLabel={t('table.label')}
    data={props.rows}
    managedColumns={managedColumns}
    getRowId={(row) => row.stored.key}
    labels={labels}
    search={{
      value: search,
      onChange: setSearch,
    }}
    stickyHeader={false}
  />
}
