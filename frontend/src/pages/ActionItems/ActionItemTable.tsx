// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { shallowEqual } from 'react-redux'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectInitiativeNamesById } from '../../store/initiativesSlice'
import { selectProjectNamesById } from '../../store/projectsSlice'

// User interface
import { Chip, Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'

// Misc
import { ACTION_ITEM_PRIORITIES, ACTION_ITEM_STATES } from '../../api/routes/actionItemRoutes'
import { useInitiativesLoader, useProjectsLoader } from '../../hooks/useServerData'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import { getActionItemViewUrl } from '../../urls'
import {
  isOverdue,
  priorityChipColors,
  priorityLabelKeys,
  stateChipColors,
  stateLabelKeys,
} from './actionItemPresentation'

type Props = {
  items: ActionItem[]
  now: number
  // Names the table for screen readers.
  label: string
  // Where column widths and order are remembered; each place the table appears has its own.
  storageId: string
  // Shows the search field and column controls, for the full list.
  isSearchable?: boolean
  // A trailing column of actions per row, such as restoring a deleted item.
  renderRowActions?: (item: ActionItem) => ReactNode
  // Columns that would only repeat the page's own subject, such as Initiatives on an
  // initiative's page. Pass a constant, so the columns are not rebuilt on every render.
  omittedColumns?: readonly ItemColumnKey[]
}

const ITEM_COLUMN_KEYS = [
  'title',
  'state',
  'priority',
  'due',
  'projects',
  'initiatives',
  'updated',
  'rowActions',
] as const
export type ItemColumnKey = typeof ITEM_COLUMN_KEYS[number]

const columnLabelKeys = {
  title: 'table.title',
  state: 'table.state',
  priority: 'table.priority',
  due: 'table.due',
  projects: 'table.projects',
  initiatives: 'table.initiatives',
  updated: 'table.updated',
  rowActions: 'common:actions.moreActions',
} as const satisfies Record<ItemColumnKey, string>

// Starting widths in pixels, summing to less than the page's content column.
const columnSizes = {
  title: 320,
  state: 120,
  priority: 120,
  due: 150,
  projects: 180,
  initiatives: 180,
  updated: 170,
  rowActions: 120,
} as const satisfies Record<ItemColumnKey, number>

// Action items in a table. Clicking a row opens the item. The list page, an initiative's
// members, and a project's items all use it, each remembering its own column layout.
export function ActionItemTable(props: Props) {
  const { t, i18n } = useTranslation([ 'actionItems', 'common' ])
  const navigate = useNavigate()
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')
  useProjectsLoader()
  useInitiativesLoader()
  const projectNames = useAppSelector(selectProjectNamesById, shallowEqual)
  const initiativeNames = useAppSelector(selectInitiativeNamesById, shallowEqual)
  const renderRowActions = props.renderRowActions
  const omittedColumns = props.omittedColumns

  // The overdue items as one string, so the columns rebuild (which resets the table's
  // state) only when an item turns overdue, not on every tick of the clock.
  const overdueIds = props.items
    .filter((item) => isOverdue(item, props.now))
    .map((item) => item.id)
    .join(',')

  const managedColumns = useMemo(() => {
    // Timestamps arrive as UTC; this is where they become the viewer's local time.
    const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium' })
    const dateTimeFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })
    const overdue = new Set(overdueIds.split(','))

    // Deleted projects and initiatives leave no name behind, so they are skipped.
    function namesText(ids: string[], names: Record<string, string>) {
      return ids
        .map((id) => names[id])
        .filter(Boolean)
        .join(', ')
    }

    const renderCell = {
      title: (item: ActionItem) => <Link
        href={getActionItemViewUrl(item.id)}
        className='font-medium text-link no-underline hover:underline'
      >{
        item.title
      }</Link>,
      state: (item: ActionItem) => <Chip size='sm' variant='soft' color={stateChipColors[item.state]}>{
        t(stateLabelKeys[item.state])
      }</Chip>,
      priority: (item: ActionItem) => <Chip size='sm' variant='soft' color={priorityChipColors[item.priority]}>{
        t(priorityLabelKeys[item.priority])
      }</Chip>,
      due: (item: ActionItem) => {
        if (!item.dueAt) {
          return ''
        }
        return <span className={overdue.has(item.id)
          ? 'text-danger'
          : undefined}
        >{
          dateFormatter.format(new Date(item.dueAt))
        }</span>
      },
      projects: (item: ActionItem) => <span className='line-clamp-2'>{namesText(item.projectIds, projectNames)}</span>,
      initiatives: (item: ActionItem) => <span className='line-clamp-2'>{
        namesText(item.initiativeIds, initiativeNames)
      }</span>,
      updated: (item: ActionItem) => dateTimeFormatter.format(new Date(item.updatedAt)),
      // Actions sit apart from the row's click, so pressing one never opens the item.
      rowActions: (item: ActionItem) => <div data-no-row-highlight className='flex justify-end'>{
        renderRowActions?.(item)
      }</div>,
    } satisfies Record<ItemColumnKey, (item: ActionItem) => unknown>

    // Search matches what the user sees.
    const searchValue = {
      title: (item: ActionItem) => item.title,
      state: (item: ActionItem) => t(stateLabelKeys[item.state]),
      priority: (item: ActionItem) => t(priorityLabelKeys[item.priority]),
      due: (item: ActionItem) => item.dueAt
        ? dateFormatter.format(new Date(item.dueAt))
        : '',
      projects: (item: ActionItem) => namesText(item.projectIds, projectNames),
      initiatives: (item: ActionItem) => namesText(item.initiativeIds, initiativeNames),
      updated: (item: ActionItem) => dateTimeFormatter.format(new Date(item.updatedAt)),
      rowActions: null,
    } satisfies Record<ItemColumnKey, ((item: ActionItem) => string) | null>

    // Sorting follows meaning rather than text: states in their lifecycle order, priority
    // from urgent down, dates in time with undated items last.
    const sortValue = {
      title: (item: ActionItem) => item.title.toLocaleLowerCase(i18n.language),
      state: (item: ActionItem) => ACTION_ITEM_STATES.indexOf(item.state),
      priority: (item: ActionItem) => ACTION_ITEM_PRIORITIES.indexOf(item.priority),
      due: (item: ActionItem) => item.dueAt
        ? Date.parse(item.dueAt)
        : Number.POSITIVE_INFINITY,
      projects: (item: ActionItem) => namesText(item.projectIds, projectNames),
      initiatives: (item: ActionItem) => namesText(item.initiativeIds, initiativeNames),
      updated: (item: ActionItem) => Date.parse(item.updatedAt),
      rowActions: null,
    } satisfies Record<ItemColumnKey, ((item: ActionItem) => string | number) | null>

    // The actions column appears only where there are actions to show.
    const columnKeys = ITEM_COLUMN_KEYS.filter((columnKey) => {
      if (columnKey === 'rowActions') {
        return Boolean(renderRowActions)
      }
      return !omittedColumns?.includes(columnKey)
    })

    return createManagedColumns<ActionItem, ItemColumnKey>({
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
  }, [ t, i18n.language, projectNames, initiativeNames, renderRowActions, omittedColumns, overdueIds ])

  return <SmartTable
    ids={{
      tableElementId: props.storageId,
      tableLocalStorageId: props.storageId,
    }}
    className='[&_tbody_tr]:cursor-pointer'
    tableAriaLabel={props.label}
    data={props.items}
    managedColumns={managedColumns}
    getRowId={(item) => item.id}
    labels={labels}
    search={props.isSearchable
      ? { value: search, onChange: setSearch }
      : undefined}
    toolbar={props.isSearchable
      ? undefined
      : { show: false }}
    enableSorting
    // A row click opens the item; nothing is ever shown as highlighted.
    enableRowHighlight
    highlightedRowId={null}
    onHighlightedRowChange={(itemId) => {
      if (itemId) {
        navigate(getActionItemViewUrl(itemId))
      }
    }}
    stickyHeader={false}
  />
}
