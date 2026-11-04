// Copyright © 2026 Jalapeno Labs

import type { CsvValue, DataTableAction, DataTableSelectionAction, SmartTableFilters } from '@jalapenolabs/uikit'
import type { ReactNode } from 'react'
import type { Initiative } from '../../api/routes/initiativeRoutes'

// Core
import { useMemo } from 'react'
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
import { DateText } from '../../format/DateText'
import { formatDate } from '../../format/dates'
import { useProjectsLoader } from '../../hooks/useServerData'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import { getInitiativeViewUrl } from '../../urls'
import { initiativeStateChipColors, initiativeStateLabelKeys } from './initiativePresentation'

type Props = {
  initiatives: Initiative[]
  label: string
  // Where column widths and order are remembered; each place the table appears has its own.
  storageId: string
  // The page's titlebar, where the page's own list renders its search, filters, Columns, and
  // export (`usePageToolsContainer`). Without it the table is one section of its page: no
  // search, and its Columns and export sit just above it.
  controlsContainer?: Element | null
  // The filters drawer and its tags, for the page's own list.
  filters?: SmartTableFilters<Initiative>
  // Said in place of the rows when there are none. The table itself stays, so its controls
  // and filter tags stay too, and a filter that matches nothing can be taken off again.
  emptyNotice?: ReactNode
  // Shown over a row while it is hovered, such as restoring a deleted initiative.
  rowActions?: DataTableAction<Initiative>[]
  // Turns on multi-select, with these actions on the floating selection bar.
  selectionActions?: DataTableSelectionAction<Initiative>[]
}

const INITIATIVE_COLUMN_KEYS = [ 'name', 'state', 'progress', 'target', 'projects' ] as const
type InitiativeColumnKey = typeof INITIATIVE_COLUMN_KEYS[number]

const columnLabelKeys = {
  name: 'table.name',
  state: 'table.state',
  progress: 'table.progress',
  target: 'table.target',
  projects: 'table.projects',
} as const satisfies Record<InitiativeColumnKey, string>

// Starting widths in pixels, summing to less than the page's content column.
const columnSizes = {
  name: 280,
  state: 130,
  progress: 240,
  target: 150,
  projects: 200,
} as const satisfies Record<InitiativeColumnKey, number>

// Initiatives in a table, each with its progress as counts. Clicking a row opens it.
export function InitiativeTable(props: Props) {
  const { t, i18n } = useTranslation([ 'initiatives', 'common' ])
  const navigate = useNavigate()
  const labels = useSmartTableLabels()
  const isPageList = props.controlsContainer !== undefined
  const hasInitiatives = props.initiatives.length > 0
  useProjectsLoader()
  const projectNames = useAppSelector(selectProjectNamesById, shallowEqual)

  const managedColumns = useMemo(() => {
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
      return formatDate(initiative.targetAt)
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
      target: (initiative: Initiative) => initiative.targetAt && <DateText
        isDay
        isFocusable={false}
        value={initiative.targetAt}
      />,
      projects: projectsText,
    } satisfies Record<InitiativeColumnKey, (initiative: Initiative) => unknown>

    const searchValue = {
      name: (initiative: Initiative) => initiative.name,
      state: (initiative: Initiative) => t(initiativeStateLabelKeys[initiative.state]),
      progress: progressText,
      target: targetText,
      projects: projectsText,
    } satisfies Record<InitiativeColumnKey, (initiative: Initiative) => string>

    // Progress sorts by how much is left, the question a list of goals is usually asked.
    const sortValue = {
      name: (initiative: Initiative) => initiative.name.toLocaleLowerCase(i18n.language),
      state: (initiative: Initiative) => INITIATIVE_STATES.indexOf(initiative.state),
      progress: (initiative: Initiative) => initiative.progress.total - initiative.progress.resolved,
      target: (initiative: Initiative) => initiative.targetAt
        ? Date.parse(initiative.targetAt)
        : Number.POSITIVE_INFINITY,
      projects: projectsText,
    } satisfies Record<InitiativeColumnKey, (initiative: Initiative) => string | number>

    // The export holds what the table shows: the state by name, progress as its counts in words
    // (a spreadsheet reads "3/10" as a date), and the target day itself.
    const exportValue = {
      name: (initiative: Initiative) => initiative.name,
      state: (initiative: Initiative) => t(initiativeStateLabelKeys[initiative.state]),
      progress: progressText,
      target: (initiative: Initiative) => initiative.targetAt,
      projects: projectsText,
    } satisfies Record<InitiativeColumnKey, (initiative: Initiative) => CsvValue>

    return createManagedColumns<Initiative, InitiativeColumnKey>({
      columnKeys: INITIATIVE_COLUMN_KEYS,
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
  }, [ t, i18n.language, projectNames ])

  return <>
    <SmartTable
      ids={{
        tableElementId: props.storageId,
        tableLocalStorageId: props.storageId,
      }}
      className={hasInitiatives
        ? '[&_tbody_tr]:cursor-pointer'
        : 'hidden'}
      tableAriaLabel={props.label}
      data={props.initiatives}
      managedColumns={managedColumns}
      getRowId={(initiative) => initiative.id}
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
      selectionActions={props.selectionActions}
      csvExport={{ fileName: 'initiatives' }}
      pagination={{ show: true }}
      enableSorting
      // A row click opens the initiative; nothing is ever shown as highlighted. Since a click goes
      // somewhere, a clamped cell opens in full on a double-click instead of expanding the row.
      cellOverflow='modal'
      enableRowHighlight
      highlightedRowId={null}
      onHighlightedRowChange={(initiativeId) => {
        if (initiativeId) {
          navigate(getInitiativeViewUrl(initiativeId))
        }
      }}
      stickyHeader={false}
    />
    {!hasInitiatives && props.emptyNotice}
  </>
}
