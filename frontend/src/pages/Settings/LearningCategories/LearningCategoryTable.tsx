// Copyright © 2026 Jalapeno Labs

import type { DataTableAction } from '@jalapenolabs/uikit'
import type { Row } from '@tanstack/react-table'
import type { LearningCategory } from '../../../api/routes/learningCategoryRoutes'
import type { PresentedLearningCategory } from '../../Knowledge/Learnings/learningPresentation'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip, Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuArrowDown, LuArrowUp, LuTrash2 } from 'react-icons/lu'

// Misc
import { formatNumber } from '../../../format/numbers'
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import { getLearningCategoryEditUrl } from '../../../urls'

// A category as a row: as people read it, with what the API holds of it.
export type LearningCategoryRow = {
  presented: PresentedLearningCategory
  stored: LearningCategory
}

type Props = {
  // In the order they are offered: built-in ones first, then the workspace's own.
  rows: LearningCategoryRow[]
  onMove: (category: LearningCategory, direction: 'up' | 'down') => Promise<void>
  onRemove: (category: LearningCategory) => void
}

const CATEGORY_COLUMN_KEYS = [ 'category', 'type', 'description', 'learnings' ] as const
type CategoryColumnKey = typeof CATEGORY_COLUMN_KEYS[number]

const columnLabelKeys = {
  category: 'table.category',
  type: 'table.type',
  description: 'table.description',
  learnings: 'table.learnings',
} as const satisfies Record<CategoryColumnKey, string>

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  category: 220,
  type: 130,
  description: 420,
  learnings: 110,
} as const satisfies Record<CategoryColumnKey, number>

// Every category in the order agents and people are offered them. The order is the point, so
// the table does not sort; the workspace's own move with their row actions. Built-in ones are
// fixed, so they have none.
export function LearningCategoryTable(props: Props) {
  const { t } = useTranslation([ 'learningCategories', 'common' ])
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    const renderCell = {
      // A workspace category's editing is its name's link, so it opens in a new tab too.
      category: (row: LearningCategoryRow) => <div className='flex flex-col'>
        {row.stored.builtIn
          ? <span className='font-medium'>{row.presented.label}</span>
          : <Link
            href={getLearningCategoryEditUrl(row.stored.key)}
            className='font-medium text-link no-underline hover:underline'
          >{
              row.presented.label
            }</Link>}
        <code className='font-mono text-xs opacity-60'>{row.stored.key}</code>
      </div>,
      type: (row: LearningCategoryRow) => row.stored.builtIn
        ? <Chip size='sm' variant='soft'>{t('table.builtIn')}</Chip>
        : <span className='opacity-80'>{t('table.custom')}</span>,
      description: (row: LearningCategoryRow) => <span className='opacity-80'>{
        row.presented.description
      }</span>,
      learnings: (row: LearningCategoryRow) => <span className='opacity-80'>{
        formatNumber(row.stored.learningCount)
      }</span>,
    } satisfies Record<CategoryColumnKey, (row: LearningCategoryRow) => unknown>

    const searchValue = {
      category: (row: LearningCategoryRow) => `${row.presented.label} ${row.stored.key}`,
      type: (row: LearningCategoryRow) => row.stored.builtIn
        ? t('table.builtIn')
        : t('table.custom'),
      description: (row: LearningCategoryRow) => row.presented.description,
      learnings: null,
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
        header: columnLabel,
        accessorFn: searchValue[columnKey] ?? undefined,
        enableSorting: false,
        size: columnSizes[columnKey],
        cell: ({ row }) => renderCell[columnKey](row.original),
      }),
    })
  }, [ t ])

  const rowActions = useMemo((): DataTableAction<LearningCategoryRow>[] => {
    // The workspace's own, in order, which is what moving up and down moves among.
    const customKeys: string[] = []
    for (const row of props.rows) {
      if (!row.stored.builtIn) {
        customKeys.push(row.stored.key)
      }
    }

    const isCustom = (row: Row<LearningCategoryRow>) => !row.original.stored.builtIn
    const place = (row: Row<LearningCategoryRow>) => customKeys.indexOf(row.original.stored.key)

    return [
      {
        id: 'moveUp',
        label: t('actions.moveUp'),
        icon: <LuArrowUp className='size-4' aria-hidden />,
        isVisible: isCustom,
        isDisabled: (row) => place(row) <= 0,
        onPress: (row) => props.onMove(row.original.stored, 'up'),
      },
      {
        id: 'moveDown',
        label: t('actions.moveDown'),
        icon: <LuArrowDown className='size-4' aria-hidden />,
        isVisible: isCustom,
        isDisabled: (row) => place(row) >= customKeys.length - 1,
        onPress: (row) => props.onMove(row.original.stored, 'down'),
      },
      {
        id: 'remove',
        label: t('actions.remove'),
        icon: <LuTrash2 className='size-4' aria-hidden />,
        isDanger: true,
        isVisible: isCustom,
        onPress: (row) => props.onRemove(row.original.stored),
      },
    ]
  }, [ t, props.rows, props.onMove, props.onRemove ])

  return <SmartTable
    ids={{
      tableElementId: 'learning-categories-table',
      tableLocalStorageId: 'elysium.settings.learningCategories.table.v2',
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
    rowActions={rowActions}
    pagination={{ show: true }}
    stickyHeader={false}
  />
}
