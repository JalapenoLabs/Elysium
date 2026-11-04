// Copyright © 2026 Jalapeno Labs

import type { CsvValue, DataTableAction } from '@jalapenolabs/uikit'
import type { ReactNode } from 'react'
import type { Meeting } from '../../../api/routes/meetingRoutes'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { meetingUpserted } from '../../../store/meetingsSlice'

// User interface
import { Chip, Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuExternalLink, LuRotateCcw } from 'react-icons/lu'
import { ConnectorLogo } from '../Connectors/ConnectorLogo'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { MEETING_STATES, retryMeeting } from '../../../api/routes/meetingRoutes'
import { DateText } from '../../../format/DateText'
import { formatDateTime } from '../../../format/dates'
import { formatDuration } from '../../../format/durations'
import { formatNumber } from '../../../format/numbers'
import { useSmartTableLabels } from '../../../hooks/useSmartTableLabels'
import { getConnectorUrl } from '../../../urls'
import { toast } from '../../../toast/toast'
import { meetingStateChipColors, meetingStateLabelKeys } from './meetingsPresentation'

type Props = {
  // Newest first.
  meetings: Meeting[]
  // The page's titlebar, where the table renders its search, Columns, and export.
  controlsContainer: Element | null
  // Said in place of the rows when there are none. The table itself stays, so its controls
  // stay in the titlebar too.
  emptyNotice: ReactNode
}

const MEETING_COLUMN_KEYS = [ 'meeting', 'source', 'state', 'started', 'duration', 'topics' ] as const
type MeetingColumnKey = typeof MEETING_COLUMN_KEYS[number]

const columnLabelKeys = {
  meeting: 'table.meeting',
  source: 'table.source',
  state: 'table.state',
  started: 'table.started',
  duration: 'table.duration',
  topics: 'table.topics',
} as const satisfies Record<MeetingColumnKey, string>

// Starting widths in pixels, summing to less than the content column at md.
const columnSizes = {
  meeting: 240,
  source: 140,
  state: 220,
  started: 130,
  duration: 120,
  topics: 100,
} as const satisfies Record<MeetingColumnKey, number>

// uikit holds a column to 160 pixels unless it sets its own floor, and most of these need less.
const COLUMN_MIN_SIZE = 100

// The person's recent meetings in a table: each one's title (opening it at its source), the
// source it came from, how far its processing got and why it failed, when it was, how long it
// ran, and how many topics Elysia found. A failed one can be tried again from its row. No row
// opens a page of its own, so a click expands the row, which is how a long failure reads in
// full.
export function MeetingTable(props: Props) {
  const { t, i18n } = useTranslation([ 'meetings', 'connectors', 'common' ])
  const dispatch = useAppDispatch()
  const labels = useSmartTableLabels()
  const hasMeetings = props.meetings.length > 0

  const managedColumns = useMemo(() => {
    function titleText(meeting: Meeting) {
      return meeting.title || t('list.untitled')
    }
    function sourceText(meeting: Meeting) {
      return t(`connectors:kinds.${meeting.sourceKind}.name`)
    }
    function stateText(meeting: Meeting) {
      return t(meetingStateLabelKeys[meeting.state])
    }
    // Under the state: why the last attempt failed, or, for a finished meeting, that it was
    // published from its transcript alone.
    function stateNote(meeting: Meeting) {
      if (meeting.lastError) {
        return meeting.lastError
      }
      if (meeting.state === 'done' && !meeting.hasRecording) {
        return t('list.noRecording')
      }
      return ''
    }
    // A source may not say how long the meeting ran.
    function durationText(meeting: Meeting) {
      if (meeting.durationMs === null) {
        return ''
      }
      return formatDuration(meeting.durationMs)
    }
    // Elysia has found no topics until she has read the meeting.
    function topicsText(meeting: Meeting) {
      if (meeting.segmentCount === null) {
        return ''
      }
      return formatNumber(meeting.segmentCount)
    }

    const renderCell = {
      meeting: (meeting: Meeting) => meeting.url
        ? <Link
          href={meeting.url}
          target='_blank'
          rel='noreferrer'
          className='font-medium text-link no-underline hover:underline'
        >
          <span>{titleText(meeting)}</span>
          <LuExternalLink className='ml-1 inline size-3.5 shrink-0' aria-hidden />
        </Link>
        : <span className='font-medium'>{titleText(meeting)}</span>,
      // The connector is gone once removed; its kind still names where the meeting came from.
      source: (meeting: Meeting) => <span className='inline-flex items-center gap-1.5'>
        <ConnectorLogo kind={meeting.sourceKind} className='size-4' />
        {meeting.connectorId
          ? <Link href={getConnectorUrl(meeting.connectorId)} className='text-inherit no-underline hover:underline'>{
            sourceText(meeting)
          }</Link>
          : <span>{sourceText(meeting)}</span>}
      </span>,
      state: (meeting: Meeting) => {
        const note = stateNote(meeting)
        const noteTone = meeting.lastError
          ? 'text-danger'
          : 'opacity-70'
        return <div>
          <Chip size='sm' variant='soft' color={meetingStateChipColors[meeting.state]}>{
            stateText(meeting)
          }</Chip>
          {note && <div className={`mt-1 text-xs ${noteTone}`}>{note}</div>}
        </div>
      },
      started: (meeting: Meeting) => <DateText value={meeting.startedAt} isFocusable={false} />,
      duration: durationText,
      topics: topicsText,
    } satisfies Record<MeetingColumnKey, (meeting: Meeting) => unknown>

    // Search matches what the user sees.
    const searchValue = {
      meeting: titleText,
      source: sourceText,
      state: (meeting: Meeting) => `${stateText(meeting)} ${stateNote(meeting)}`,
      started: (meeting: Meeting) => formatDateTime(meeting.startedAt),
      duration: durationText,
      topics: topicsText,
    } satisfies Record<MeetingColumnKey, (meeting: Meeting) => string>

    // Words sort without regard to case, the state through processing, and the rest by value,
    // with a length or a count not known yet last.
    const sortValue = {
      meeting: (meeting: Meeting) => titleText(meeting).toLocaleLowerCase(i18n.language),
      source: (meeting: Meeting) => sourceText(meeting).toLocaleLowerCase(i18n.language),
      state: (meeting: Meeting) => MEETING_STATES.indexOf(meeting.state),
      started: (meeting: Meeting) => Date.parse(meeting.startedAt),
      duration: (meeting: Meeting) => meeting.durationMs ?? Number.POSITIVE_INFINITY,
      topics: (meeting: Meeting) => meeting.segmentCount ?? Number.POSITIVE_INFINITY,
    } satisfies Record<MeetingColumnKey, (meeting: Meeting) => string | number>

    // The export holds the state by name, the start as its instant, the length as the table
    // writes it, and the topics as a number.
    const exportValue = {
      meeting: titleText,
      source: sourceText,
      state: stateText,
      started: (meeting: Meeting) => meeting.startedAt,
      duration: durationText,
      topics: (meeting: Meeting) => meeting.segmentCount,
    } satisfies Record<MeetingColumnKey, (meeting: Meeting) => CsvValue>

    return createManagedColumns<Meeting, MeetingColumnKey>({
      columnKeys: MEETING_COLUMN_KEYS,
      getColumnLabel: (columnKey) => t(columnLabelKeys[columnKey]),
      getSearchKey: (columnKey) => columnKey,
      getSearchValue: (columnKey) => searchValue[columnKey],
      getExportValue: (columnKey) => exportValue[columnKey],
      createColumnDef: ({ columnKey, columnId, columnLabel }) => ({
        id: columnId,
        header: columnLabel,
        accessorFn: sortValue[columnKey],
        size: columnSizes[columnKey],
        minSize: COLUMN_MIN_SIZE,
        cell: ({ row }) => renderCell[columnKey](row.original),
      }),
    })
  }, [ t, i18n.language ])

  // A failed meeting resumes from the step it failed at. The promise spins the button until the
  // API answers.
  const rowActions: DataTableAction<Meeting>[] = [{
    id: 'retry',
    label: t('list.retry'),
    icon: <LuRotateCcw className='size-4' aria-hidden />,
    isVisible: (row) => row.original.state === 'failed',
    onPress: async (row) => {
      try {
        const response = await retryMeeting(row.original.id)
        dispatch(meetingUpserted(response.meeting))
        toast.success(t('list.retried'))
      }
      catch (error) {
        console.debug('MeetingTable failed to retry a meeting', { error, meetingId: row.original.id })
        toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
      }
    },
  }]

  return <>
    <SmartTable
      ids={{
        tableElementId: 'meetings-table',
        tableLocalStorageId: 'elysium.settings.meetings.table.v1',
      }}
      className={hasMeetings
        ? undefined
        : 'hidden'}
      tableAriaLabel={t('tabs.meetings')}
      data={props.meetings}
      managedColumns={managedColumns}
      getRowId={(meeting) => meeting.id}
      labels={labels}
      controls={{ container: props.controlsContainer }}
      rowActions={rowActions}
      csvExport={{ fileName: 'meetings' }}
      pagination={{ show: true }}
      enableSorting
      stickyHeader={false}
    />
    {!hasMeetings && props.emptyNotice}
  </>
}
