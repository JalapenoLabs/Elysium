// Copyright © 2026 Jalapeno Labs

import type { CsvValue } from '@jalapenolabs/uikit'
import type { KnowledgePageSummary } from '../../../api/routes/pageRoutes'
import type { PageTree } from './pageTree'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { DateText } from '../../../format/DateText'
import { LabelLink } from './LabelLink'

// Misc
import { formatDateTime } from '../../../format/dates'
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import { usePageToolsContainer } from '../../../layout/page/pageTools'
import { getKnowledgePageUrl } from '../../../urls'
import { findPageAncestors } from './pageTree'

type Props = {
  label: string
  pages: KnowledgePageSummary[]
  tree: PageTree
}

const LABEL_PAGE_COLUMN_KEYS = [ 'title', 'location', 'labels', 'updatedAt' ] as const
type LabelPageColumnKey = typeof LABEL_PAGE_COLUMN_KEYS[number]

// Starting widths in pixels, summing to less than the knowledge area's content column.
const columnSizes = {
  title: 260,
  location: 220,
  labels: 190,
  updatedAt: 150,
} as const satisfies Record<LabelPageColumnKey, number>

// The pages carrying a label: each opening the page, where it sits in the tree, the labels it
// carries beside this one, and when it last changed. Its search and export sit in the titlebar.
export function LabelPagesTable(props: Props) {
  const { t, i18n } = useTranslation([ 'knowledge', 'common' ])
  const labels = useSmartTableLabels()
  const toolsContainer = usePageToolsContainer()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    const columnLabels = {
      title: t('labels.table.page'),
      location: t('labels.table.location'),
      labels: t('labels.table.labels'),
      updatedAt: t('labels.table.updated'),
    } as const satisfies Record<LabelPageColumnKey, string>

    const location = (page: KnowledgePageSummary) => findPageAncestors(props.tree, page.id)
      .map((ancestor) => ancestor.title)
      .join(' / ')
      || t('labels.table.topOfTree')
    const otherLabels = (page: KnowledgePageSummary) => page.labels.filter((label) => label !== props.label)

    const renderCell = {
      title: (page: KnowledgePageSummary) => <Link href={getKnowledgePageUrl(page.id)} className='text-link'>{
        page.title
      }</Link>,
      location: (page: KnowledgePageSummary) => <span className='opacity-80'>{location(page)}</span>,
      labels: (page: KnowledgePageSummary) => <div className='flex flex-wrap gap-1'>{
        otherLabels(page).map((label) => <LabelLink key={label} label={label} />)
      }</div>,
      updatedAt: (page: KnowledgePageSummary) => <DateText
        value={page.updatedAt}
        isFocusable={false}
        className='opacity-80'
      />,
    } satisfies Record<LabelPageColumnKey, (page: KnowledgePageSummary) => unknown>

    // Search matches what the reader sees.
    const searchValue = {
      title: (page: KnowledgePageSummary) => page.title,
      location,
      labels: (page: KnowledgePageSummary) => otherLabels(page).join(' '),
      updatedAt: (page: KnowledgePageSummary) => formatDateTime(page.updatedAt),
    } satisfies Record<LabelPageColumnKey, (page: KnowledgePageSummary) => string>

    // Sorting follows the text, except the date, which sorts in time.
    const sortValue = {
      title: (page: KnowledgePageSummary) => page.title.toLocaleLowerCase(i18n.language),
      location: (page: KnowledgePageSummary) => location(page).toLocaleLowerCase(i18n.language),
      labels: (page: KnowledgePageSummary) => otherLabels(page).join(' '),
      updatedAt: (page: KnowledgePageSummary) => Date.parse(page.updatedAt),
    } satisfies Record<LabelPageColumnKey, (page: KnowledgePageSummary) => string | number>

    // The export holds what the table shows, with the instant itself for the date.
    const exportValue = {
      title: (page: KnowledgePageSummary) => page.title,
      location,
      labels: (page: KnowledgePageSummary) => otherLabels(page).join(', '),
      updatedAt: (page: KnowledgePageSummary) => page.updatedAt,
    } satisfies Record<LabelPageColumnKey, (page: KnowledgePageSummary) => CsvValue>

    return createManagedColumns<KnowledgePageSummary, LabelPageColumnKey>({
      columnKeys: LABEL_PAGE_COLUMN_KEYS,
      getColumnLabel: (columnKey) => columnLabels[columnKey],
      getSearchKey: (columnKey) => columnKey,
      getSearchValue: (columnKey) => searchValue[columnKey],
      getExportValue: (columnKey) => exportValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => ({
        id: columnId,
        header: columnLabel,
        accessorFn: sortValue[columnKey],
        size: columnSizes[columnKey],
        cell: ({ row }) => renderCell[columnKey](row.original),
      }),
    })
  }, [ t, i18n.language, props.label, props.tree ])

  if (!props.pages.length) {
    return <EmptyNotice>{t('labels.unmatched')}</EmptyNotice>
  }

  return <SmartTable
    ids={{
      tableElementId: 'label-pages-table',
      tableLocalStorageId: 'elysium.knowledge.label-pages.table.v1',
    }}
    tableAriaLabel={t('labels.table.label', { label: props.label })}
    data={props.pages}
    managedColumns={managedColumns}
    getRowId={(page) => page.id}
    labels={labels}
    search={{
      value: search,
      onChange: setSearch,
    }}
    controls={{ container: toolsContainer }}
    csvExport={{ fileName: `pages-labeled-${props.label}` }}
    pagination={{ show: true }}
    enableSorting
    stickyHeader={false}
  />
}
