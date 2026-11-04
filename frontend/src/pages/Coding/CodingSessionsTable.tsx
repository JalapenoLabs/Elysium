// Copyright © 2026 Jalapeno Labs

import type { CodingSession } from '../../api/routes/codingSessionRoutes'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// User interface
import { Chip, Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'

// Misc
import { DateText } from '../../format/DateText'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import { getCodingSessionUrl } from '../../urls'
import {
  SESSION_NUMBER_COLUMN_SIZING,
  activityStateChipColors,
  activityStateLabelKeys,
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
}

const SESSION_COLUMN_KEYS = [ 'number', 'title', 'state', 'lastActivity' ] as const
type SessionColumnKey = typeof SESSION_COLUMN_KEYS[number]

const columnLabelKeys = {
  number: 'sessions.number',
  title: 'sessions.title',
  state: 'sessions.state',
  lastActivity: 'sessions.lastActivity',
} as const satisfies Record<SessionColumnKey, string>

// Starting widths in pixels, summing to less than the page's content column.
const columnSizes = {
  number: SESSION_NUMBER_COLUMN_SIZING.size,
  title: 280,
  state: 160,
  lastActivity: 220,
} as const satisfies Record<SessionColumnKey, number>

// A list of coding sessions outside the Coding page, such as a project's or an action
// item's. Clicking one opens its conversation on the Coding page.
export function CodingSessionsTable(props: Props) {
  const { t } = useTranslation('coding')
  const navigate = useNavigate()
  const labels = useSmartTableLabels()

  const managedColumns = useMemo(() => {
    function stateLabel(session: CodingSession) {
      return t(activityStateLabelKeys[session.activity.state])
    }
    function lastActivity(session: CodingSession) {
      const lastActivityAt = session.activity.lastActivityAt
      if (!lastActivityAt) {
        return t('sessions.never')
      }
      return <DateText value={lastActivityAt} isFocusable={false} />
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
      state: (session: CodingSession) => <Chip
        size='sm'
        variant='soft'
        color={activityStateChipColors[session.activity.state]}
      >{
        stateLabel(session)
      }</Chip>,
      lastActivity,
    } satisfies Record<SessionColumnKey, (session: CodingSession) => unknown>

    // Each sorts by its value, which is also what the export holds: the instant for last activity.
    const sortValue = {
      number: (session: CodingSession) => String(session.id),
      title: (session: CodingSession) => session.title,
      state: stateLabel,
      lastActivity: (session: CodingSession) => session.activity.lastActivityAt ?? '',
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
  }, [ t ])

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
    csvExport={{ fileName: 'coding-sessions' }}
    pagination={{ show: true }}
    enableSorting
    // A row click opens the session; nothing is ever shown as highlighted. Since a click goes
    // somewhere, a clamped cell opens in full on a double-click instead of expanding the row.
    cellOverflow='modal'
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
