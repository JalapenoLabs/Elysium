// Copyright © 2026 Jalapeno Labs

import type { Learning } from '../../../api/routes/learningRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip, Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { EmptyNotice } from '../../../components/EmptyNotice'

// Misc
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import { useUserNames } from '../../../hooks/useUserNames'
import { getLearningUrl } from '../../../urls'
import { categoryLabelKeys } from './learningPresentation'

type Props = {
  learnings: Learning[]
  // The deleted tab dates each row by its deletion rather than its last change.
  isDeletedList: boolean
}

const LEARNING_COLUMN_KEYS = [ 'title', 'category', 'savedBy', 'changedAt' ] as const
type LearningColumnKey = typeof LEARNING_COLUMN_KEYS[number]

// Starting widths in pixels, summing to less than the settings content column.
const columnSizes = {
  title: 400,
  category: 140,
  savedBy: 220,
  changedAt: 180,
} as const satisfies Record<LearningColumnKey, number>

// Learnings in a table: the takeaway, opening the learning; its category; who first saved it and
// through which agent; and when it last changed, or was deleted.
export function LearningTable(props: Props) {
  const { t, i18n } = useTranslation([ 'learnings', 'common' ])
  const labels = useSmartTableLabels()
  const userNames = useUserNames()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium' })
    const columnLabels = {
      title: t('table.title'),
      category: t('table.category'),
      savedBy: t('table.savedBy'),
      changedAt: props.isDeletedList
        ? t('table.deletedAt')
        : t('table.updated'),
    } as const satisfies Record<LearningColumnKey, string>

    const changedAt = (learning: Learning) => learning.deletedAt ?? learning.updatedAt
    const savedBy = (learning: Learning) => userNames[learning.createdBy] ?? t('common:table.someone')

    const renderCell = {
      title: (learning: Learning) => <Link href={getLearningUrl(learning.id)} className='line-clamp-2 text-link'>{
        learning.title
      }</Link>,
      category: (learning: Learning) => <Chip size='sm' variant='soft'>{
        t(categoryLabelKeys[learning.category])
      }</Chip>,
      savedBy: (learning: Learning) => <div className='flex flex-col'>
        <span>{savedBy(learning)}</span>
        {learning.agent && <span className='text-xs opacity-60'>{
          t('table.savedByAgent', { agent: learning.agent.name ?? t('agent.unnamed') })
        }</span>}
      </div>,
      changedAt: (learning: Learning) => <span className='opacity-80'>{
        dateFormatter.format(new Date(changedAt(learning)))
      }</span>,
    } satisfies Record<LearningColumnKey, (learning: Learning) => unknown>

    // What search matches and sorting orders by: what the reader sees, except the date, which
    // is the instant itself so the column sorts in time order.
    const searchValue = {
      title: (learning: Learning) => learning.title,
      category: (learning: Learning) => t(categoryLabelKeys[learning.category]),
      savedBy,
      changedAt,
    } satisfies Record<LearningColumnKey, (learning: Learning) => string>

    return createManagedColumns<Learning, LearningColumnKey>({
      columnKeys: LEARNING_COLUMN_KEYS,
      getColumnLabel: (columnKey) => columnLabels[columnKey],
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
  }, [ t, i18n.language, userNames, props.isDeletedList ])

  if (!props.learnings.length) {
    return <EmptyNotice>{
      props.isDeletedList
        ? t('table.emptyDeleted')
        : t('table.empty')
    }</EmptyNotice>
  }

  return <SmartTable
    ids={{
      tableElementId: props.isDeletedList
        ? 'deleted-learnings-table'
        : 'learnings-table',
      tableLocalStorageId: props.isDeletedList
        ? 'elysium.settings.learnings.deleted.table.v1'
        : 'elysium.settings.learnings.table.v1',
    }}
    tableAriaLabel={t('table.label')}
    data={props.learnings}
    managedColumns={managedColumns}
    getRowId={(learning) => learning.id}
    labels={labels}
    search={{
      value: search,
      onChange: setSearch,
    }}
    enableSorting
    stickyHeader={false}
  />
}
