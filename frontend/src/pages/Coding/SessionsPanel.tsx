// Copyright © 2026 Jalapeno Labs

import type { DataTableAction, DataTableSelectionAction, SmartTableFilter } from '@jalapenolabs/uikit'
import type { RowSelectionState } from '@tanstack/react-table'
import type { SessionSummary } from './sessionPresentation'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'

// Redux
import { shallowEqual } from 'react-redux'
import { selectCodingSessions } from '../../store/codingSessionsSlice'
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { selectAllProjects, selectProjectNamesById } from '../../store/projectsSlice'
import { selectSessionsView, sessionsViewChanged } from '../../store/sessionsViewSlice'

// User interface
import { buttonVariants, Chip, Link, Spinner } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuMessagesSquare, LuPencil, LuPlus, LuTrash2 } from 'react-icons/lu'
import { SessionsToolbar } from './SessionsToolbar'
import { SessionTiles } from './SessionTiles'

// Misc
import { ACTIVITY_STATES } from '../../api/routes/codingSessionRoutes'
import { formatShortDateTime } from '../../format/dates'
import { useCodingSessionsLoader, useProjectsLoader } from '../../hooks/useServerData'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import { UrlTree } from '../../urls'
import { useCodingActions } from './codingActionsContext'
import {
  describeSession,
  searchSessionSummaries,
  SESSION_NUMBER_COLUMN_SIZING,
  activityStateChipColors,
  activityStateLabelKeys,
} from './sessionPresentation'
import {
  filterSessionSummaries,
  fromSessionFilterValues,
  readSessionFilters,
  SESSION_FILTER_IDS,
  toSessionFilterValues,
  writeSessionFilters,
} from './sessionFilters'

const SESSION_COLUMN_KEYS = [
  'number',
  'title',
  'project',
  'state',
  'lastActivity',
] as const
type SessionColumnKey = typeof SESSION_COLUMN_KEYS[number]

const columnLabelKeys = {
  number: 'sessions.number',
  title: 'sessions.title',
  project: 'sessions.project',
  state: 'sessions.state',
  lastActivity: 'sessions.lastActivity',
} as const satisfies Record<SessionColumnKey, string>

// Starting widths in pixels, sized for the panel's default half of the workspace.
const columnSizes = {
  number: SESSION_NUMBER_COLUMN_SIZING.size,
  title: 260,
  project: 170,
  state: 120,
  lastActivity: 170,
} as const satisfies Record<SessionColumnKey, number>

// What each column sorts by, which is also what the export holds. Last activity sorts by the
// instant, not the words shown, so an August session never follows a September one.
const sortValue = {
  number: (summary: SessionSummary) => String(summary.session.id),
  title: (summary: SessionSummary) => summary.session.title,
  project: (summary: SessionSummary) => summary.projectName,
  state: (summary: SessionSummary) => summary.stateLabel,
  lastActivity: (summary: SessionSummary) => summary.session.activity.lastActivityAt ?? '',
} as const satisfies Record<SessionColumnKey, (summary: SessionSummary) => string>

// The overview panel: every coding session, with what it is doing now, as a table or as
// tiles. Opening one focuses its conversation panel. Search and filters list the same
// sessions in both views; the filters live in the Coding page's address.
export function SessionsPanel() {
  const { t } = useTranslation([ 'coding', 'common' ])
  const actions = useCodingActions()
  const labels = useSmartTableLabels()
  const dispatch = useAppDispatch()
  const [ search, setSearch ] = useState('')
  const [ searchParams, setSearchParams ] = useSearchParams()
  const filters = readSessionFilters(searchParams)
  // The toolbar's slot for the table's filters, Columns, and export.
  const [ controlsContainer, setControlsContainer ] = useState<HTMLDivElement | null>(null)
  const [ rowSelection, setRowSelection ] = useState<RowSelectionState>({})
  const view = useAppSelector(selectSessionsView)

  const sessions = useAppSelector(selectCodingSessions)
  const sessionsStatus = useCodingSessionsLoader()
  const projectNames = useAppSelector(selectProjectNamesById, shallowEqual)
  const projects = useAppSelector(selectAllProjects)
  const projectsStatus = useProjectsLoader()

  // Both views and the search read the same summaries, derived once per change.
  const summaries = useMemo(() => {
    const context = {
      projectNames,
      formatInstant: (instant: string) => formatShortDateTime(instant),
    }
    return sessions.map((session) => describeSession(session, t, context))
  }, [ sessions, projectNames, t ])

  const listedSummaries = filterSessionSummaries(searchSessionSummaries(summaries, search), filters)

  // None carries `matches`: the panel applies them itself, for the tiles as for the table.
  const filterDefinitions = useMemo((): SmartTableFilter<SessionSummary>[] => [
    {
      id: SESSION_FILTER_IDS.states,
      label: t('sessions.state'),
      kind: 'multiSelect',
      options: ACTIVITY_STATES.map((state) => ({ value: state, label: t(activityStateLabelKeys[state]) })),
    },
    {
      id: SESSION_FILTER_IDS.project,
      label: t('sessions.project'),
      kind: 'select',
      options: projects.map((project) => ({ value: project.id, label: project.name })),
    },
  ], [ t, projects ])

  // The columns read only the summaries, so activity updates never rebuild them, which
  // would reset the table's sorting.
  const managedColumns = useMemo(() => {
    const renderCell = {
      number: (summary: SessionSummary) => <span className='block text-right tabular-nums'>{
        summary.session.id
      }</span>,
      title: (summary: SessionSummary) => <Link
        className='cursor-pointer font-medium text-link no-underline hover:underline'
        onPress={() => actions.openSession(summary.session)}
      >{
        summary.session.title
      }</Link>,
      project: (summary: SessionSummary) => summary.projectName,
      state: (summary: SessionSummary) => <Chip
        size='sm'
        variant='soft'
        color={activityStateChipColors[summary.state]}
      >{
        summary.stateLabel
      }</Chip>,
      lastActivity: (summary: SessionSummary) => summary.lastActivity,
    } satisfies Record<SessionColumnKey, (summary: SessionSummary) => unknown>

    return createManagedColumns<SessionSummary, SessionColumnKey>({
      columnKeys: SESSION_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      // The panel's toolbar searches, for both views; the table's own search is hidden.
      getSearchKey: () => null,
      getSearchValue: (columnKey) => sortValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => ({
        id: columnId,
        header: columnLabel,
        accessorFn: sortValue[columnKey],
        size: columnSizes[columnKey],
        cell: ({ row }) => renderCell[columnKey](row.original),
      }),
      columnDefOverrides: {
        number: ({ columnId, columnLabel }) => ({
          id: columnId,
          header: () => <span className='numeric-column-header'>{columnLabel}</span>,
          // Sorted as a number, so session 10 follows session 9.
          accessorFn: (summary) => summary.session.id,
          ...SESSION_NUMBER_COLUMN_SIZING,
          cell: ({ row }) => renderCell.number(row.original),
        }),
      },
    })
  }, [ t, actions ])

  // The tiles show the same three in their own menu (`SessionRowActions`).
  const rowActions = useMemo((): DataTableAction<SessionSummary>[] => [
    {
      id: 'open',
      label: t('sessions.open'),
      icon: <LuMessagesSquare className='size-4' aria-hidden />,
      onPress: (row) => actions.openSession(row.original.session),
    },
    {
      id: 'rename',
      label: t('common:actions.rename'),
      icon: <LuPencil className='size-4' aria-hidden />,
      onPress: (row) => actions.renameSession(row.original.session),
    },
    {
      id: 'delete',
      label: t('common:actions.delete'),
      icon: <LuTrash2 className='size-4' aria-hidden />,
      isDanger: true,
      onPress: (row) => actions.deleteSession(row.original.session),
    },
  ], [ t, actions ])

  // Deleting can't be undone, so the page confirms once for the whole selection.
  const selectionActions = useMemo((): DataTableSelectionAction<SessionSummary>[] => [{
    id: 'delete',
    label: t('common:actions.delete'),
    icon: <LuTrash2 className='size-4' aria-hidden />,
    isDanger: true,
    onPress: (rows) => actions.deleteSessions(rows.map((summary) => summary.session)),
  }], [ t, actions ])

  if (sessionsStatus === 'loading') {
    return <div className='grid h-full place-items-center'>
      <Spinner />
    </div>
  }

  if (sessionsStatus === 'failed') {
    return <p className='p-6 text-center text-sm text-danger'>{
      t('sessions.loadError')
    }</p>
  }

  if (!sessions.length) {
    return <div className='grid h-full place-items-center p-6'>
      <div className='max-w-sm text-center'>
        <SessionsEmptyState
          hasProjects={projectsStatus === 'loaded' && Object.keys(projectNames).length > 0}
        />
      </div>
    </div>
  }

  const isTable = view === 'table'

  // A container, so the toolbar and tiles fit the panel's width rather than the window's.
  return <div className='@container h-full overflow-auto p-4'>
    <SessionsToolbar
      search={search}
      onSearchChange={setSearch}
      resultsCount={listedSummaries.length}
      view={view}
      onViewChange={(nextView) => dispatch(sessionsViewChanged(nextView))}
      controlsRef={setControlsContainer}
    />

    {/* The table stays in both views, hidden behind the tiles or the empty message, since its
        filters drawer, their tags, and the export serve the tiles too. Columns are its own. */}
    <SmartTable
      ids={{
        tableElementId: 'coding-sessions-table',
        tableLocalStorageId: 'elysium.coding.sessions.table.v3',
      }}
      className={isTable && listedSummaries.length
        ? undefined
        : 'hidden'}
      tableAriaLabel={t('sessions.label')}
      data={listedSummaries}
      managedColumns={managedColumns}
      getRowId={(summary) => String(summary.session.id)}
      labels={labels}
      search={{ show: false }}
      controls={{ container: controlsContainer }}
      columnControls={{ show: isTable }}
      filters={{
        definitions: filterDefinitions,
        values: toSessionFilterValues(filters),
        onValuesChange: (values) => setSearchParams(
          writeSessionFilters(searchParams, fromSessionFilterValues(values)),
          { replace: true },
        ),
      }}
      rowActions={rowActions}
      // The tiles have no checkboxes, so selecting belongs to the table view.
      enableSelection={isTable}
      rowSelection={rowSelection}
      onRowSelectionChange={setRowSelection}
      selectionActions={selectionActions}
      csvExport={{ fileName: 'coding-sessions' }}
      pagination={{ show: true }}
      enableSorting
      stickyHeader={false}
    />

    {!listedSummaries.length && <p className='py-10 text-center text-sm opacity-70'>{
      t('sessions.noMatches')
    }</p>}

    {listedSummaries.length > 0 && !isTable && <SessionTiles
      summaries={listedSummaries}
    />}
  </div>
}

type SessionsEmptyStateProps = {
  hasProjects: boolean
}

// A session needs a project; point at it when there is none.
function SessionsEmptyState(props: SessionsEmptyStateProps) {
  const { t } = useTranslation('coding')

  if (!props.hasProjects) {
    return <>
      <p className='relaxed text-sm opacity-70'>{t('sessions.noProjects')}</p>
      <Link href={UrlTree.projects} className='text-sm text-link'>{t('sessions.openProjects')}</Link>
    </>
  }

  return <>
    <p className='relaxed text-sm opacity-70'>{t('sessions.empty')}</p>
    <Link href={UrlTree.codingNew} className={buttonVariants({ size: 'sm' })}>
      <LuPlus className='size-4' aria-hidden />
      <span>{t('newSession')}</span>
    </Link>
  </>
}
