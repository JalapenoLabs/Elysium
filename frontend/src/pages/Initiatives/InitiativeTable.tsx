// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { Initiative } from '../../api/routes/initiativeRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { shallowEqual } from 'react-redux'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectProjectNamesById } from '../../store/projectsSlice'

// User interface
import { Chip, Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { InitiativeProgressSummary } from './InitiativeProgressSummary'

// Misc
import { INITIATIVE_STATES } from '../../api/routes/initiativeRoutes'
import { useProjectsLoader } from '../../hooks/useServerData'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import { getInitiativeViewUrl } from '../../urls'
import { initiativeStateChipColors, initiativeStateLabelKeys } from './initiativePresentation'

type Props = {
  initiatives: Initiative[]
  label: string
  // Where column widths and order are remembered; each place the table appears has its own.
  storageId: string
  isSearchable?: boolean
  // A trailing column of actions per row, such as restoring a deleted initiative.
  renderRowActions?: (initiative: Initiative) => ReactNode
}

const INITIATIVE_COLUMN_KEYS = [ 'name', 'state', 'progress', 'target', 'projects', 'rowActions' ] as const
type InitiativeColumnKey = typeof INITIATIVE_COLUMN_KEYS[number]

const columnLabelKeys = {
  name: 'table.name',
  state: 'table.state',
  progress: 'table.progress',
  target: 'table.target',
  projects: 'table.projects',
  rowActions: 'common:actions.moreActions',
} as const satisfies Record<InitiativeColumnKey, string>

// Starting widths in pixels, summing to less than the page's content column.
const columnSizes = {
  name: 280,
  state: 130,
  progress: 240,
  target: 150,
  projects: 200,
  rowActions: 120,
} as const satisfies Record<InitiativeColumnKey, number>

// Initiatives in a table, each with its progress as counts. Clicking a row opens it.
export function InitiativeTable(props: Props) {
  const { t, i18n } = useTranslation([ 'initiatives', 'common' ])
  const navigate = useNavigate()
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')
  useProjectsLoader()
  const projectNames = useAppSelector(selectProjectNamesById, shallowEqual)
  const renderRowActions = props.renderRowActions

  const managedColumns = useMemo(() => {
    const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium' })

    // Deleted projects leave no name behind, so they are skipped.
    function projectsText(initiative: Initiative) {
      return initiative.projectIds
        .map((projectId) => projectNames[projectId])
        .filter(Boolean)
        .join(', ')
    }
    function targetText(initiative: Initiative) {
      if (!initiative.targetAt) {
        return ''
      }
      return dateFormatter.format(new Date(initiative.targetAt))
    }
    function progressText(initiative: Initiative) {
      return t('progress.counts', initiative.progress)
    }

    const renderCell = {
      name: (initiative: Initiative) => <Link
        href={getInitiativeViewUrl(initiative.id)}
        className='font-medium text-link no-underline hover:underline'
      >{
        initiative.name
      }</Link>,
      state: (initiative: Initiative) => <Chip
        size='sm'
        variant='soft'
        color={initiativeStateChipColors[initiative.state]}
      >{
        t(initiativeStateLabelKeys[initiative.state])
      }</Chip>,
      progress: (initiative: Initiative) => <InitiativeProgressSummary progress={initiative.progress} />,
      target: targetText,
      projects: (initiative: Initiative) => <span className='line-clamp-2'>{projectsText(initiative)}</span>,
      rowActions: (initiative: Initiative) => <div data-no-row-highlight className='flex justify-end'>{
        renderRowActions?.(initiative)
      }</div>,
    } satisfies Record<InitiativeColumnKey, (initiative: Initiative) => unknown>

    const searchValue = {
      name: (initiative: Initiative) => initiative.name,
      state: (initiative: Initiative) => t(initiativeStateLabelKeys[initiative.state]),
      progress: progressText,
      target: targetText,
      projects: projectsText,
      rowActions: null,
    } satisfies Record<InitiativeColumnKey, ((initiative: Initiative) => string) | null>

    // Progress sorts by how much is left, the question a list of goals is usually asked.
    const sortValue = {
      name: (initiative: Initiative) => initiative.name.toLocaleLowerCase(i18n.language),
      state: (initiative: Initiative) => INITIATIVE_STATES.indexOf(initiative.state),
      progress: (initiative: Initiative) => initiative.progress.total - initiative.progress.resolved,
      target: (initiative: Initiative) => initiative.targetAt
        ? Date.parse(initiative.targetAt)
        : Number.POSITIVE_INFINITY,
      projects: projectsText,
      rowActions: null,
    } satisfies Record<InitiativeColumnKey, ((initiative: Initiative) => string | number) | null>

    const columnKeys = renderRowActions
      ? INITIATIVE_COLUMN_KEYS
      : INITIATIVE_COLUMN_KEYS.filter((columnKey) => columnKey !== 'rowActions')

    return createManagedColumns<Initiative, InitiativeColumnKey>({
      columnKeys,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: (columnKey) => searchValue[columnKey]
        ? columnKey
        : null,
      getSearchValue: (columnKey) => searchValue[columnKey],
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
  }, [ t, i18n.language, projectNames, renderRowActions ])

  return <SmartTable
    ids={{
      tableElementId: props.storageId,
      tableLocalStorageId: props.storageId,
    }}
    className='[&_tbody_tr]:cursor-pointer'
    tableAriaLabel={props.label}
    data={props.initiatives}
    managedColumns={managedColumns}
    getRowId={(initiative) => initiative.id}
    labels={labels}
    search={props.isSearchable
      ? { value: search, onChange: setSearch }
      : undefined}
    toolbar={props.isSearchable
      ? undefined
      : { show: false }}
    enableSorting
    // A row click opens the initiative; nothing is ever shown as highlighted.
    enableRowHighlight
    highlightedRowId={null}
    onHighlightedRowChange={(initiativeId) => {
      if (initiativeId) {
        navigate(getInitiativeViewUrl(initiativeId))
      }
    }}
    stickyHeader={false}
  />
}
