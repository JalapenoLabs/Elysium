// Copyright © 2026 Jalapeno Labs

import type { CodingSession } from '../../api/routes/codingSessionRoutes'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { shallowEqual } from 'react-redux'
import { useAppSelector } from '../../store/hooks'
import { selectSatelliteNamesById } from '../../store/satellitesSlice'

// User interface
import { Chip, Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'

// Misc
import { useSatellitesLoader } from '../../hooks/useServerData'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import { getCodingSessionUrl } from '../../urls'
import { threadStateChipColors, threadStateLabelKeys } from '../Coding/sessionPresentation'

type Props = {
  sessions: CodingSession[]
}

const SESSION_COLUMN_KEYS = [ 'title', 'satellite', 'state', 'lastActivity' ] as const
type SessionColumnKey = typeof SESSION_COLUMN_KEYS[number]

const columnLabelKeys = {
  title: 'coding:sessions.title',
  satellite: 'coding:sessions.satellite',
  state: 'coding:sessions.state',
  lastActivity: 'coding:sessions.lastActivity',
} as const satisfies Record<SessionColumnKey, string>

// Starting widths in pixels, summing to less than the page's content column.
const columnSizes = {
  title: 380,
  satellite: 200,
  state: 160,
  lastActivity: 220,
} as const satisfies Record<SessionColumnKey, number>

// A project's coding sessions. Clicking one opens its conversation on the Coding page.
export function ProjectSessionsTable(props: Props) {
  const { t, i18n } = useTranslation([ 'projects', 'coding' ])
  const navigate = useNavigate()
  const labels = useSmartTableLabels()
  useSatellitesLoader()
  const satelliteNames = useAppSelector(selectSatelliteNamesById, shallowEqual)

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
      return t(threadStateLabelKeys[session.thread?.state ?? 'unknown'], { ns: 'coding' })
    }
    function lastActivityText(session: CodingSession) {
      const lastActivityAt = session.thread?.lastActivityAt
      if (!lastActivityAt) {
        return t('coding:sessions.never')
      }
      return dateFormatter.format(new Date(lastActivityAt))
    }

    const renderCell = {
      title: (session: CodingSession) => <Link
        href={getCodingSessionUrl(session.id)}
        className='font-medium text-link no-underline hover:underline'
      >{
        session.title
      }</Link>,
      satellite: satelliteName,
      state: (session: CodingSession) => {
        const state = session.thread?.state ?? 'unknown'
        return <Chip size='sm' variant='soft' color={threadStateChipColors[state]}>{
          stateLabel(session)
        }</Chip>
      },
      lastActivity: lastActivityText,
    } satisfies Record<SessionColumnKey, (session: CodingSession) => unknown>

    const sortValue = {
      title: (session: CodingSession) => session.title,
      satellite: satelliteName,
      state: stateLabel,
      lastActivity: (session: CodingSession) => session.thread?.lastActivityAt ?? '',
    } satisfies Record<SessionColumnKey, (session: CodingSession) => string>

    return createManagedColumns<CodingSession, SessionColumnKey>({
      columnKeys: SESSION_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: () => null,
      getSearchValue: (columnKey) => sortValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => ({
        id: columnId,
        header: columnLabel,
        accessorFn: sortValue[columnKey],
        size: columnSizes[columnKey],
        cell: ({ row }) => renderCell[columnKey](row.original),
      }),
    })
  }, [ t, i18n.language, satelliteNames ])

  if (!props.sessions.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('page.sessionsEmpty')
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: 'project-sessions-table',
      tableLocalStorageId: 'elysium.projects.sessions.table',
    }}
    className='[&_tbody_tr]:cursor-pointer'
    tableAriaLabel={t('page.sessionsHeading')}
    data={props.sessions}
    managedColumns={managedColumns}
    getRowId={(session) => session.id}
    labels={labels}
    toolbar={{ show: false }}
    enableSorting
    // A row click opens the session; nothing is ever shown as highlighted.
    enableRowHighlight
    highlightedRowId={null}
    onHighlightedRowChange={(sessionId) => {
      if (sessionId) {
        navigate(getCodingSessionUrl(sessionId))
      }
    }}
    stickyHeader={false}
  />
}
