// Copyright © 2026 Jalapeno Labs

import type { SessionSummary } from './sessionPresentation'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { shallowEqual } from 'react-redux'
import { selectCodingSessions } from '../../store/codingSessionsSlice'
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { selectProjectNamesById } from '../../store/projectsSlice'
import { selectSessionsView, sessionsViewChanged } from '../../store/sessionsViewSlice'

// User interface
import { Button, Chip, Link, Spinner } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuPlus } from 'react-icons/lu'
import { SessionRowActions } from './SessionRowActions'
import { SessionsToolbar } from './SessionsToolbar'
import { SessionTiles } from './SessionTiles'

// Misc
import { useCodingSessionsLoader, useProjectsLoader } from '../../hooks/useServerData'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import { UrlTree } from '../../urls'
import { useCodingActions } from './codingActionsContext'
import {
  describeSession,
  searchSessionSummaries,
  SESSION_NUMBER_COLUMN_SIZING,
  activityStateChipColors,
} from './sessionPresentation'

const SESSION_COLUMN_KEYS = [
  'number',
  'title',
  'project',
  'state',
  'lastActivity',
  'rowActions',
] as const
type SessionColumnKey = typeof SESSION_COLUMN_KEYS[number]

const columnLabelKeys = {
  number: 'sessions.number',
  title: 'sessions.title',
  project: 'sessions.project',
  state: 'sessions.state',
  lastActivity: 'sessions.lastActivity',
  rowActions: 'common:actions.moreActions',
} as const satisfies Record<SessionColumnKey, string>

// Starting widths in pixels, sized for the panel's default half of the workspace.
const columnSizes = {
  number: SESSION_NUMBER_COLUMN_SIZING.size,
  title: 260,
  project: 170,
  state: 120,
  lastActivity: 170,
  rowActions: 56,
} as const satisfies Record<SessionColumnKey, number>

// What each sortable column sorts by. Last activity sorts by the instant, not the words
// shown, so an August session never follows a September one.
const sortValue = {
  number: (summary: SessionSummary) => String(summary.session.id),
  title: (summary: SessionSummary) => summary.session.title,
  project: (summary: SessionSummary) => summary.projectName,
  state: (summary: SessionSummary) => summary.stateLabel,
  lastActivity: (summary: SessionSummary) => summary.session.activity.lastActivityAt ?? '',
  rowActions: null,
} as const satisfies Record<SessionColumnKey, ((summary: SessionSummary) => string) | null>

// The overview panel: every coding session, with what it is doing now, as a table or as
// tiles. Opening one focuses its conversation panel.
export function SessionsPanel() {
  const { t, i18n } = useTranslation([ 'coding', 'common' ])
  const actions = useCodingActions()
  const labels = useSmartTableLabels()
  const dispatch = useAppDispatch()
  const [ search, setSearch ] = useState('')
  const view = useAppSelector(selectSessionsView)

  const sessions = useAppSelector(selectCodingSessions)
  const sessionsStatus = useCodingSessionsLoader()
  const projectNames = useAppSelector(selectProjectNamesById, shallowEqual)
  const projectsStatus = useProjectsLoader()

  // Both views and the search read the same summaries, derived once per change.
  const summaries = useMemo(() => {
    // Timestamps arrive as UTC; this is where they become the viewer's local time.
    const dateFormatter = new Intl.DateTimeFormat(i18n.language, {
      dateStyle: 'medium',
      timeStyle: 'short',
    })
    const context = {
      projectNames,
      formatInstant: (instant: string) => dateFormatter.format(new Date(instant)),
    }
    return sessions.map((session) => describeSession(session, t, context))
  }, [ sessions, projectNames, t, i18n.language ])

  const listedSummaries = useMemo(
    () => searchSessionSummaries(summaries, search),
    [ summaries, search ],
  )

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
      rowActions: (summary: SessionSummary) => <SessionRowActions
        session={summary.session}
      />,
    } satisfies Record<SessionColumnKey, (summary: SessionSummary) => unknown>

    return createManagedColumns<SessionSummary, SessionColumnKey>({
      columnKeys: SESSION_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      // The panel's toolbar searches, for both views; the table's own search is hidden.
      getSearchKey: () => null,
      getSearchValue: (columnKey) => sortValue[columnKey],
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
          onCreateSession={() => actions.createSession()}
        />
      </div>
    </div>
  }

  // A container, so the toolbar and tiles fit the panel's width rather than the window's.
  return <div className='@container h-full overflow-auto p-4'>
    <SessionsToolbar
      search={search}
      onSearchChange={setSearch}
      resultsCount={listedSummaries.length}
      view={view}
      onViewChange={(nextView) => dispatch(sessionsViewChanged(nextView))}
    />

    {!listedSummaries.length && <p className='py-10 text-center text-sm opacity-70'>{
      t('sessions.noMatches')
    }</p>}

    {listedSummaries.length > 0 && view === 'table' && <SmartTable
      ids={{
        tableElementId: 'coding-sessions-table',
        tableLocalStorageId: 'elysium.coding.sessions.table.v3',
      }}
      tableAriaLabel={t('sessions.label')}
      data={listedSummaries}
      managedColumns={managedColumns}
      getRowId={(summary) => String(summary.session.id)}
      labels={labels}
      toolbar={{ show: false }}
      enableSorting
      stickyHeader={false}
    />}

    {listedSummaries.length > 0 && view === 'tiles' && <SessionTiles
      summaries={listedSummaries}
    />}
  </div>
}

type SessionsEmptyStateProps = {
  hasProjects: boolean
  onCreateSession: () => void
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
    <Button size='sm' onPress={props.onCreateSession}>
      <LuPlus className='size-4' aria-hidden />
      <span>{t('newSession')}</span>
    </Button>
  </>
}
