// Copyright © 2026 Jalapeno Labs

import type { Project } from '../../api/routes/projectRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { ProjectRowActions } from './ProjectRowActions'

// Misc
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'

type Props = {
  projects: Project[]
  sessionCounts: Record<string, number>
  onEdit: (project: Project) => void
  onDelete: (project: Project) => void
}

const PROJECT_COLUMN_KEYS = [ 'name', 'sessions', 'updated', 'rowActions' ] as const
type ProjectColumnKey = typeof PROJECT_COLUMN_KEYS[number]

const columnLabelKeys = {
  name: 'table.name',
  sessions: 'table.sessions',
  updated: 'table.updated',
  rowActions: 'common:actions.moreActions',
} as const satisfies Record<ProjectColumnKey, string>

// Starting widths in pixels, summing to less than the page's content column.
const columnSizes = {
  name: 420,
  sessions: 160,
  updated: 200,
  rowActions: 64,
} as const satisfies Record<ProjectColumnKey, number>

export function ProjectTable(props: Props) {
  const { t, i18n } = useTranslation([ 'projects', 'common' ])
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  const managedColumns = useMemo(() => {
    // Timestamps arrive as UTC; this is where they become the viewer's local time.
    const dateFormatter = new Intl.DateTimeFormat(i18n.language, {
      dateStyle: 'medium',
      timeStyle: 'short',
    })
    const countFormatter = new Intl.NumberFormat(i18n.language)

    const renderCell = {
      name: (project: Project) => <div>
        <div className='font-medium'>{project.name}</div>
        {project.description && <div className='max-w-md truncate text-xs opacity-70'>{
          project.description
        }</div>}
      </div>,
      sessions: (project: Project) => countFormatter.format(props.sessionCounts[project.id] ?? 0),
      updated: (project: Project) => dateFormatter.format(new Date(project.updatedAt)),
      rowActions: (project: Project) => <ProjectRowActions
        project={project}
        onEdit={props.onEdit}
        onDelete={props.onDelete}
      />,
    } satisfies Record<ProjectColumnKey, (project: Project) => unknown>

    // Search matches names and descriptions only.
    function searchText(project: Project) {
      return `${project.name} ${project.description}`
    }

    // Counts and dates sort by their values rather than their formatted text.
    const sortValue = {
      name: searchText,
      sessions: (project: Project) => props.sessionCounts[project.id] ?? 0,
      updated: (project: Project) => project.updatedAt,
      rowActions: null,
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
  }, [ t, i18n.language, props.sessionCounts, props.onEdit, props.onDelete ])

  if (!props.projects.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('table.empty')
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: 'projects-table',
      tableLocalStorageId: 'elysium.projects.table',
    }}
    tableAriaLabel={t('table.label')}
    data={props.projects}
    managedColumns={managedColumns}
    getRowId={(project) => project.id}
    labels={labels}
    search={{
      value: search,
      onChange: setSearch,
    }}
    enableSorting
    stickyHeader={false}
  />
}
