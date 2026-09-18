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
import {
  SESSION_NUMBER_COLUMN_SIZING,
  threadStateChipColors,
  threadStateLabelKeys,
} from './sessionPresentation'

type Props = {
  sessions: CodingSession[]
  // The table's element id, and where its column layout is saved in the browser. Each
  // page keeps its own, ending in a version uikit's saved column order can move past.
  ids: {
    tableElementId: string
    tableLocalStorageId: string
  }
  ariaLabel: string
  // Shown instead of an empty table.
  emptyMessage: string
  // The columns to show, for a page narrower than the project page; every column by default.
  columnKeys?: readonly SessionColumnKey[]
}

const SESSION_COLUMN_KEYS = [ 'number', 'title', 'satellite', 'state', 'lastActivity' ] as const
export type SessionColumnKey = typeof SESSION_COLUMN_KEYS[number]

const columnLabelKeys = {
  number: 'sessions.number',
  title: 'sessions.title',
  satellite: 'sessions.satellite',
  state: 'sessions.state',
  lastActivity: 'sessions.lastActivity',
} as const satisfies Record<SessionColumnKey, string>

// Starting widths in pixels, summing to less than the page's content column.
const columnSizes = {
  number: SESSION_NUMBER_COLUMN_SIZING.size,
  title: 240,
  satellite: 200,
  state: 160,
  lastActivity: 220,
} as const satisfies Record<SessionColumnKey, number>

// A list of coding sessions outside the Coding page, such as a project's or an action
// item's. Clicking one opens its conversation on the Coding page.
export function CodingSessionsTable(props: Props) {
  const { t, i18n } = useTranslation('coding')
  const navigate = useNavigate()
  const labels = useSmartTableLabels()
  useSatellitesLoader()
  const satelliteNames = useAppSelector(selectSatelliteNamesById, shallowEqual)
  const columnKeys = props.columnKeys ?? SESSION_COLUMN_KEYS

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
      number: (session: CodingSession) => <span className='block text-right tabular-nums'>{
        session.id
      }</span>,
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
      number: (session: CodingSession) => String(session.id),
      title: (session: CodingSession) => session.title,
      satellite: satelliteName,
      state: stateLabel,
      lastActivity: (session: CodingSession) => session.thread?.lastActivityAt ?? '',
    } satisfies Record<SessionColumnKey, (session: CodingSession) => string>

    return createManagedColumns<CodingSession, SessionColumnKey>({
      columnKeys,
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
      columnDefOverrides: {
        number: ({ columnId, columnLabel }) => ({
          id: columnId,
          header: () => <span className='numeric-column-header'>{columnLabel}</span>,
          // Sorted as a number, so session 10 follows session 9.
          accessorFn: (session) => session.id,
          ...SESSION_NUMBER_COLUMN_SIZING,
          cell: ({ row }) => renderCell.number(row.original),
        }),
      },
    })
  }, [ t, i18n.language, satelliteNames, columnKeys ])

  if (!props.sessions.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      props.emptyMessage
    }</p>
  }

  return <SmartTable
    ids={props.ids}
    className='[&_tbody_tr]:cursor-pointer'
    tableAriaLabel={props.ariaLabel}
    data={props.sessions}
    managedColumns={managedColumns}
    getRowId={(session) => String(session.id)}
    labels={labels}
    toolbar={{ show: false }}
    enableSorting
    // A row click opens the session; nothing is ever shown as highlighted.
    enableRowHighlight
    highlightedRowId={null}
    onHighlightedRowChange={(sessionId) => {
      if (sessionId) {
        navigate(getCodingSessionUrl(Number(sessionId)))
      }
    }}
    stickyHeader={false}
  />
}
