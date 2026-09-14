// Copyright © 2026 Jalapeno Labs

import type { CodingSession } from '../../api/routes/codingSessionRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { shallowEqual } from 'react-redux'
import { selectAllCodingSessions } from '../../store/codingSessionsSlice'
import { useAppSelector } from '../../store/hooks'
import { selectSatelliteNamesById } from '../../store/satellitesSlice'

// User interface
import { Button, Chip, Link, Spinner } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuPlus } from 'react-icons/lu'
import { SessionRowActions } from './SessionRowActions'

// Misc
import { useCodingSessionsLoader, useSatellitesLoader } from '../../hooks/useServerData'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import { UrlTree } from '../../urls'
import { useCodingActions } from './codingActionsContext'
import { threadStateChipColors, threadStateLabelKeys } from './sessionPresentation'

const SESSION_COLUMN_KEYS = [ 'title', 'satellite', 'state', 'lastActivity', 'rowActions' ] as const
type SessionColumnKey = typeof SESSION_COLUMN_KEYS[number]

const columnLabelKeys = {
  title: 'sessions.title',
  satellite: 'sessions.satellite',
  state: 'sessions.state',
  lastActivity: 'sessions.lastActivity',
  rowActions: 'common:actions.moreActions',
} as const satisfies Record<SessionColumnKey, string>

// Starting widths in pixels, sized for the panel's default half of the workspace.
const columnSizes = {
  title: 220,
  satellite: 150,
  state: 120,
  lastActivity: 170,
  rowActions: 56,
} as const satisfies Record<SessionColumnKey, number>

// The overview panel: every coding session across every satellite, with its live
// thread state. Opening one focuses its conversation panel.
export function SessionsPanel() {
  const { t, i18n } = useTranslation([ 'coding', 'common' ])
  const actions = useCodingActions()
  const labels = useSmartTableLabels()
  const [ search, setSearch ] = useState('')

  const sessions = useAppSelector(selectAllCodingSessions)
  const sessionsStatus = useCodingSessionsLoader()
  // Satellite status polls must not rebuild the columns, which would reset the table's sorting.
  const satelliteNames = useAppSelector(selectSatelliteNamesById, shallowEqual)
  const satellitesStatus = useSatellitesLoader()

  const managedColumns = useMemo(() => {
    // Timestamps arrive as UTC; this is where they become the viewer's local time.
    const dateFormatter = new Intl.DateTimeFormat(i18n.language, {
      dateStyle: 'medium',
      timeStyle: 'short',
    })

    function satelliteName(session: CodingSession) {
      return satelliteNames[session.satelliteId] ?? ''
    }
    function stateLabel(session: CodingSession) {
      return t(threadStateLabelKeys[session.thread?.state ?? 'unknown'])
    }
    function lastActivityText(session: CodingSession) {
      const lastActivityAt = session.thread?.lastActivityAt
      if (!lastActivityAt) {
        return t('sessions.never')
      }
      return dateFormatter.format(new Date(lastActivityAt))
    }

    const renderCell = {
      title: (session: CodingSession) => <Link
        className='cursor-pointer font-medium text-link no-underline hover:underline'
        onPress={() => actions.openSession(session)}
      >{
        session.title
      }</Link>,
      satellite: satelliteName,
      state: (session: CodingSession) => {
        const state = session.thread?.state ?? 'unknown'
        return <Chip size='sm' variant='soft' color={threadStateChipColors[state]}>{
          t(threadStateLabelKeys[state])
        }</Chip>
      },
      lastActivity: lastActivityText,
      rowActions: (session: CodingSession) => <SessionRowActions
        session={session}
      />,
    } satisfies Record<SessionColumnKey, (session: CodingSession) => unknown>

    const searchValue = {
      title: (session: CodingSession) => session.title,
      satellite: satelliteName,
      state: stateLabel,
      lastActivity: lastActivityText,
      rowActions: null,
    } satisfies Record<SessionColumnKey, ((session: CodingSession) => string) | null>

    return createManagedColumns<CodingSession, SessionColumnKey>({
      columnKeys: SESSION_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: (columnKey) => searchValue[columnKey]
        ? columnKey
        : null,
      getSearchValue: (columnKey) => searchValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => {
        const toSearchText = searchValue[columnKey]
        if (!toSearchText) {
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
          accessorFn: toSearchText,
          size: columnSizes[columnKey],
          cell: ({ row }) => renderCell[columnKey](row.original),
        }
      },
    })
  }, [ t, i18n.language, satelliteNames, actions ])

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
    const hasSatellites = satellitesStatus === 'loaded' && Object.keys(satelliteNames).length > 0

    return <div className='grid h-full place-items-center p-6'>
      <div className='max-w-sm text-center'>
        <p className='relaxed text-sm opacity-70'>{
          hasSatellites
            ? t('sessions.empty')
            : t('sessions.noSatellites')
        }</p>
        {hasSatellites
          ? <Button size='sm' onPress={actions.createSession}>
            <LuPlus className='size-4' aria-hidden />
            <span>{t('newSession')}</span>
          </Button>
          : <Link href={UrlTree.settingsSatellites} className='text-sm text-link'>{
            t('sessions.openSatelliteSettings')
          }</Link>}
      </div>
    </div>
  }

  return <div className='h-full overflow-auto p-4'>
    <SmartTable
      ids={{
        tableElementId: 'coding-sessions-table',
        tableLocalStorageId: 'elysium.coding.sessions.table',
      }}
      tableAriaLabel={t('sessions.label')}
      data={sessions}
      managedColumns={managedColumns}
      getRowId={(session) => session.id}
      labels={labels}
      search={{
        value: search,
        onChange: setSearch,
      }}
      enableSorting
      stickyHeader={false}
    />
  </div>
}
