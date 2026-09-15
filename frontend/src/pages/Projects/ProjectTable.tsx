// Copyright © 2026 Jalapeno Labs

import type { OnChangeFn, SortingState } from '@tanstack/react-table'
import type { Project } from '../../api/routes/projectRoutes'
import type { ProjectSort } from './projectListing'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// User interface
import { Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { ImagePreview } from '../../components/ImagePreview'
import { ProjectCover } from './ProjectCover'

// Misc
import { getProjectCoverUrl } from '../../api/routes/projectRoutes'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import { getProjectViewUrl } from '../../urls'
import { DEFAULT_PROJECT_SORT, PROJECT_SORT_KEYS } from './projectListing'

type Props = {
  // Already searched and sorted by the page's toolbar.
  projects: Project[]
  sort: ProjectSort
  onSortChange: (sort: ProjectSort) => void
  sessionCounts: Record<string, number>
}

const PROJECT_COLUMN_KEYS = [ 'cover', 'name', 'sessions', 'updated' ] as const
type ProjectColumnKey = typeof PROJECT_COLUMN_KEYS[number]

const columnLabelKeys = {
  cover: 'table.cover',
  name: 'table.name',
  sessions: 'table.sessions',
  updated: 'table.updated',
} as const satisfies Record<ProjectColumnKey, string>

// Starting widths in pixels, summing to less than the page's content column.
const columnSizes = {
  cover: 88,
  name: 420,
  sessions: 160,
  updated: 200,
} as const satisfies Record<ProjectColumnKey, number>

// The table view. Its column headers sort through the same state as the toolbar, and a
// row opens its project.
export function ProjectTable(props: Props) {
  const { t, i18n } = useTranslation([ 'projects', 'common' ])
  const navigate = useNavigate()
  const labels = useSmartTableLabels()

  const sorting: SortingState = [{ id: props.sort.key, desc: props.sort.descending }]
  const onSortingChange: OnChangeFn<SortingState> = (updater) => {
    const next = typeof updater === 'function'
      ? updater(sorting)
      : updater
    const [ column ] = next
    // Clearing a header's sort returns to the default rather than an unsorted list.
    const sortKey = PROJECT_SORT_KEYS.find((key) => key === column?.id)
    if (!column || !sortKey) {
      props.onSortChange(DEFAULT_PROJECT_SORT)
      return
    }
    props.onSortChange({ key: sortKey, descending: column.desc })
  }

  const managedColumns = useMemo(() => {
    // Timestamps arrive as UTC; this is where they become the viewer's local time.
    const dateFormatter = new Intl.DateTimeFormat(i18n.language, {
      dateStyle: 'medium',
      timeStyle: 'short',
    })
    const countFormatter = new Intl.NumberFormat(i18n.language)

    const renderCell = {
      cover: (project: Project) => {
        const coverUrl = getProjectCoverUrl(project)
        const thumbnail = <ProjectCover src={coverUrl} name={project.name} className='w-14 rounded-md' />
        if (!coverUrl) {
          return thumbnail
        }
        // Opening the preview must not also open the project.
        return <span data-no-row-highlight='true'>
          <ImagePreview
            src={coverUrl}
            alt={t('tile.coverAlt', { name: project.name })}
            className='inline-block rounded-md'
          >
            {thumbnail}
          </ImagePreview>
        </span>
      },
      name: (project: Project) => <div>
        <Link href={getProjectViewUrl(project.id)} className='font-medium text-inherit no-underline'>{
          project.name
        }</Link>
        {project.description && <div className='max-w-md truncate text-xs opacity-70'>{
          project.description
        }</div>}
      </div>,
      sessions: (project: Project) => countFormatter.format(props.sessionCounts[project.id] ?? 0),
      updated: (project: Project) => dateFormatter.format(new Date(project.updatedAt)),
    } satisfies Record<ProjectColumnKey, (project: Project) => unknown>

    // Search matches names and descriptions only.
    function searchText(project: Project) {
      return `${project.name} ${project.description}`
    }

    // Counts and dates sort by their values rather than their formatted text.
    const sortValue = {
      cover: null,
      name: searchText,
      sessions: (project: Project) => props.sessionCounts[project.id] ?? 0,
      updated: (project: Project) => project.updatedAt,
    } satisfies Record<ProjectColumnKey, ((project: Project) => string | number) | null>

    return createManagedColumns<Project, ProjectColumnKey>({
      columnKeys: PROJECT_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: (columnKey) => columnKey === 'name'
        ? columnKey
        : null,
      getSearchValue: (columnKey) => columnKey === 'name'
        ? searchText
        : null,
      createColumnDef: ({ columnKey, columnId, columnLabel }) => {
        const toSortValue = sortValue[columnKey]
        if (!toSortValue) {
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
          accessorFn: toSortValue,
          size: columnSizes[columnKey],
          cell: ({ row }) => renderCell[columnKey](row.original),
        }
      },
    })
  }, [ t, i18n.language, props.sessionCounts ])

  return <SmartTable
    ids={{
      tableElementId: 'projects-table',
      tableLocalStorageId: 'elysium.projects.table',
    }}
    className='[&_tbody_tr]:cursor-pointer'
    tableAriaLabel={t('table.label')}
    data={props.projects}
    managedColumns={managedColumns}
    getRowId={(project) => project.id}
    labels={labels}
    search={{ show: false }}
    // The page's toolbar searches, counts, and sorts for both views.
    toolbar={{ show: false }}
    sorting={sorting}
    onSortingChange={onSortingChange}
    enableSorting
    // A row click opens the project; nothing is ever shown as highlighted.
    enableRowHighlight
    highlightedRowId={null}
    onHighlightedRowChange={(projectId) => {
      if (projectId) {
        navigate(getProjectViewUrl(projectId))
      }
    }}
    stickyHeader={false}
  />
}
