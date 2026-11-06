// Copyright © 2026 Jalapeno Labs

import type { CsvValue, DataTableAction } from '@jalapenolabs/uikit'
import type { ProjectPage } from '../../api/routes/pageRoutes'
import type { PageTree } from '../Knowledge/Pages/pageTree'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuUnlink } from 'react-icons/lu'
import { DateText } from '../../format/DateText'
import { LabelLink } from '../Knowledge/Pages/LabelLink'

// Misc
import { formatDateTime } from '../../format/dates'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import { useUserNames } from '../../hooks/useUserNames'
import { getKnowledgePageUrl } from '../../urls'
import { findPageAncestors } from '../Knowledge/Pages/pageTree'
import { usePageProjectLinks } from '../Knowledge/Pages/usePageProjectLinks'

type Props = {
  projectId: string
  projectName: string
  pages: ProjectPage[]
  tree: PageTree
  // Where the table's search, Columns, and export go.
  controlsContainer: Element | null
}

const PROJECT_PAGE_COLUMN_KEYS = [ 'title', 'location', 'labels', 'published', 'linked' ] as const
type ProjectPageColumnKey = typeof PROJECT_PAGE_COLUMN_KEYS[number]

// Starting widths in pixels, summing to less than the page's content column.
const columnSizes = {
  title: 260,
  location: 200,
  labels: 180,
  published: 170,
  linked: 170,
} as const satisfies Record<ProjectPageColumnKey, number>

// The pages linked to a project: each opening the page, where it sits in the tree, its labels, who
// last published it, and who linked it. Taking one off is the row's own action, and Undo is in its
// toast.
export function ProjectPagesTable(props: Props) {
  const { projectId, projectName } = props
  const { t, i18n } = useTranslation([ 'projects', 'common' ])
  const labels = useSmartTableLabels()
  const userNames = useUserNames()
  const links = usePageProjectLinks()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    const columnLabels = {
      title: t('docs.columns.page'),
      location: t('docs.columns.location'),
      labels: t('docs.columns.labels'),
      published: t('docs.columns.published'),
      linked: t('docs.columns.linked'),
    } as const satisfies Record<ProjectPageColumnKey, string>

    function location(page: ProjectPage) {
      return findPageAncestors(props.tree, page.id)
        .map((ancestor) => ancestor.title)
        .join(' / ')
        || t('docs.topOfTree')
    }
    function userName(userId: string) {
      return userNames[userId] ?? t('common:table.someone')
    }

    const renderCell = {
      title: (page: ProjectPage) => <Link href={getKnowledgePageUrl(page.id)} className='text-link'>{
        page.title
      }</Link>,
      location: (page: ProjectPage) => <span className='opacity-80'>{location(page)}</span>,
      labels: (page: ProjectPage) => <div className='flex flex-wrap gap-1'>{
        page.labels.map((label) => <LabelLink key={label} label={label} />)
      }</div>,
      published: (page: ProjectPage) => <div>
        <DateText value={page.publishedAt} isFocusable={false} />
        <p className='text-xs opacity-70'>{userName(page.publishedBy)}</p>
      </div>,
      linked: (page: ProjectPage) => <div>
        <DateText value={page.linkedAt} isFocusable={false} />
        <p className='text-xs opacity-70'>{userName(page.linkedBy)}</p>
      </div>,
    } satisfies Record<ProjectPageColumnKey, (page: ProjectPage) => unknown>

    // Search matches what the reader sees.
    const searchValue = {
      title: (page: ProjectPage) => page.title,
      location,
      labels: (page: ProjectPage) => page.labels.join(' '),
      published: (page: ProjectPage) => `${formatDateTime(page.publishedAt)} ${userName(page.publishedBy)}`,
      linked: (page: ProjectPage) => `${formatDateTime(page.linkedAt)} ${userName(page.linkedBy)}`,
    } satisfies Record<ProjectPageColumnKey, (page: ProjectPage) => string>

    // Sorting follows the text, except the dates, which sort in time.
    const sortValue = {
      title: (page: ProjectPage) => page.title.toLocaleLowerCase(i18n.language),
      location: (page: ProjectPage) => location(page).toLocaleLowerCase(i18n.language),
      labels: (page: ProjectPage) => page.labels.join(' '),
      published: (page: ProjectPage) => Date.parse(page.publishedAt),
      linked: (page: ProjectPage) => Date.parse(page.linkedAt),
    } satisfies Record<ProjectPageColumnKey, (page: ProjectPage) => string | number>

    // The export holds what the table shows, with the instant itself for each date.
    const exportValue = {
      title: (page: ProjectPage) => page.title,
      location,
      labels: (page: ProjectPage) => page.labels.join(', '),
      published: (page: ProjectPage) => page.publishedAt,
      linked: (page: ProjectPage) => page.linkedAt,
    } satisfies Record<ProjectPageColumnKey, (page: ProjectPage) => CsvValue>

    return createManagedColumns<ProjectPage, ProjectPageColumnKey>({
      columnKeys: PROJECT_PAGE_COLUMN_KEYS,
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
  }, [ t, i18n.language, props.tree, userNames ])

  const rowActions: DataTableAction<ProjectPage>[] = [{
    id: 'unlink',
    label: t('docs.unlink'),
    icon: <LuUnlink className='size-4' aria-hidden />,
    onPress: (row) => links.unlink({
      pageId: row.original.id,
      pageTitle: row.original.title,
      projectId,
      projectName,
    }),
  }]

  return <SmartTable
    ids={{
      tableElementId: 'project-pages-table',
      tableLocalStorageId: 'elysium.projects.pages.table.v1',
    }}
    tableAriaLabel={t('docs.label')}
    data={props.pages}
    managedColumns={managedColumns}
    getRowId={(page) => page.id}
    labels={labels}
    search={{
      value: search,
      onChange: setSearch,
    }}
    controls={{ container: props.controlsContainer }}
    rowActions={rowActions}
    csvExport={{ fileName: `${projectName}-docs` }}
    pagination={{ show: true }}
    enableSorting
    stickyHeader={false}
  />
}
