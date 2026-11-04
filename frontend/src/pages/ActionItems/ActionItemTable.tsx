// Copyright © 2026 Jalapeno Labs

import type { CsvValue, DataTableAction, DataTableSelectionAction, SmartTableFilters } from '@jalapenolabs/uikit'
import type { RowSelectionState } from '@tanstack/react-table'
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
import { DateText } from '../../format/DateText'
import { formatDate, formatDateTime } from '../../format/dates'
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
  // The page's titlebar, where the page's own list renders its search, filters, Columns, and
  // export (`usePageToolsContainer`). Without it the table is one section of its page: no
  // search, and its Columns and export sit just above it.
  controlsContainer?: Element | null
  // The filters drawer and its tags, for the page's own list.
  filters?: SmartTableFilters<ActionItem>
  // Said in place of the rows when there are none. The table itself stays, so its controls
  // and filter tags stay too, and a filter that matches nothing can be taken off again.
  emptyNotice?: ReactNode
  // Shown over a row while it is hovered, such as restoring a deleted item.
  rowActions?: DataTableAction<ActionItem>[]
  // Turns on multi-select, with these actions on the floating selection bar.
  selectionActions?: DataTableSelectionAction<ActionItem>[]
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
} as const satisfies Record<ItemColumnKey, number>

// Action items in a table. Clicking a row opens the item. The list page, an initiative's
// members, and a project's items all use it, each remembering its own column layout.
export function ActionItemTable(props: Props) {
  const { t, i18n } = useTranslation([ 'actionItems', 'common' ])
  const navigate = useNavigate()
  const labels = useSmartTableLabels()
  const isPageList = props.controlsContainer !== undefined
  // The kit draws the checkboxes and the selection bar; the selection itself is held here.
  const [ rowSelection, setRowSelection ] = useState<RowSelectionState>({})
  const hasItems = props.items.length > 0
  useProjectsLoader()
  useInitiativesLoader()
  const projectNames = useAppSelector(selectProjectNamesById, shallowEqual)
  const initiativeNames = useAppSelector(selectInitiativeNamesById, shallowEqual)
  const omittedColumns = props.omittedColumns

  // The overdue items as one string, so the columns rebuild (which resets the table's
  // state) only when an item turns overdue, not on every tick of the clock.
  const overdueIds = props.items
    .filter((item) => isOverdue(item, props.now))
    .map((item) => item.id)
    .join(',')

  const managedColumns = useMemo(() => {
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
        return <DateText
          isDay
          isFocusable={false}
          value={item.dueAt}
          className={overdue.has(item.id)
            ? 'text-danger'
            : undefined}
        />
      },
      projects: (item: ActionItem) => namesText(item.projectIds, projectNames),
      initiatives: (item: ActionItem) => namesText(item.initiativeIds, initiativeNames),
      updated: (item: ActionItem) => <DateText value={item.updatedAt} isFocusable={false} />,
    } satisfies Record<ItemColumnKey, (item: ActionItem) => unknown>

    // Search matches what the user sees.
    const searchValue = {
      title: (item: ActionItem) => item.title,
      state: (item: ActionItem) => t(stateLabelKeys[item.state]),
      priority: (item: ActionItem) => t(priorityLabelKeys[item.priority]),
      due: (item: ActionItem) => item.dueAt
        ? formatDate(item.dueAt)
        : '',
      projects: (item: ActionItem) => namesText(item.projectIds, projectNames),
      initiatives: (item: ActionItem) => namesText(item.initiativeIds, initiativeNames),
      updated: (item: ActionItem) => formatDateTime(item.updatedAt),
    } satisfies Record<ItemColumnKey, (item: ActionItem) => string>

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
    } satisfies Record<ItemColumnKey, (item: ActionItem) => string | number>

    // The export holds what the table shows, in full: states and priorities by name, the
    // instants themselves for dates, and every project and initiative.
    const exportValue = {
      title: (item: ActionItem) => item.title,
      state: (item: ActionItem) => t(stateLabelKeys[item.state]),
      priority: (item: ActionItem) => t(priorityLabelKeys[item.priority]),
      due: (item: ActionItem) => item.dueAt,
      projects: (item: ActionItem) => namesText(item.projectIds, projectNames),
      initiatives: (item: ActionItem) => namesText(item.initiativeIds, initiativeNames),
      updated: (item: ActionItem) => item.updatedAt,
    } satisfies Record<ItemColumnKey, (item: ActionItem) => CsvValue>

    const columnKeys = ITEM_COLUMN_KEYS.filter((columnKey) => !omittedColumns?.includes(columnKey))

    return createManagedColumns<ActionItem, ItemColumnKey>({
      columnKeys,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
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
  }, [ t, i18n.language, projectNames, initiativeNames, omittedColumns, overdueIds ])

  return <>
    <SmartTable
      ids={{
        tableElementId: props.storageId,
        tableLocalStorageId: props.storageId,
      }}
      className={hasItems
        ? '[&_tbody_tr]:cursor-pointer'
        : 'hidden'}
      tableAriaLabel={props.label}
      data={props.items}
      managedColumns={managedColumns}
      getRowId={(item) => item.id}
      labels={labels}
      controls={isPageList
        ? { container: props.controlsContainer ?? null }
        : undefined}
      toolbar={isPageList
        ? undefined
        : { show: false }}
      filters={props.filters}
      rowActions={props.rowActions}
      enableSelection={Boolean(props.selectionActions?.length)}
      rowSelection={rowSelection}
      onRowSelectionChange={setRowSelection}
      selectionActions={props.selectionActions}
      csvExport={{ fileName: 'action-items' }}
      pagination={{ show: true }}
      enableSorting
      // A row click opens the item; nothing is ever shown as highlighted. Since a click goes
      // somewhere, a clamped cell opens in full on a double-click instead of expanding the row.
      cellOverflow='modal'
      enableRowHighlight
      highlightedRowId={null}
      onHighlightedRowChange={(itemId) => {
        if (itemId) {
          navigate(getActionItemViewUrl(itemId))
        }
      }}
      stickyHeader={false}
    />
    {!hasItems && props.emptyNotice}
  </>
}
