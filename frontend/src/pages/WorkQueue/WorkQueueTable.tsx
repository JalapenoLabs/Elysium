// Copyright © 2026 Jalapeno Labs

import type { CsvValue, DataTableAction } from '@jalapenolabs/uikit'
import type { WorkQueueEntry } from '../../api/routes/workQueueRoutes'
import type { WorkBatchAction } from './workQueuePresentation'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { shallowEqual } from 'react-redux'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectProjectNamesById } from '../../store/projectsSlice'

// User interface
import { Chip, Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'

// Misc
import { DateText } from '../../format/DateText'
import { formatDateTime, formatShortDateTime } from '../../format/dates'
import { useNow } from '../../hooks/useNow'
import { useProjectsLoader } from '../../hooks/useServerData'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import { getProjectViewUrl, getWorkBatchViewUrl } from '../../urls'
import {
  batchPriorityChipColors,
  batchPriorityLabelKeys,
  batchStateChipColors,
  batchStateLabelKeys,
} from '../Projects/projectManagerPresentation'
import { useWorkBatchActions } from './useWorkBatchActions'
import {
  actionIcons,
  actionLabelKeys,
  availableActions,
  DANGEROUS_ACTIONS,
} from './workQueuePresentation'

// The queue's lists, one per tab.
export type WorkQueueList = 'proposed' | 'queued' | 'running' | 'scheduled' | 'paused'

type Props = {
  list: WorkQueueList
  entries: WorkQueueEntry[]
}

const COLUMN_KEYS = [ 'position', 'title', 'project', 'priority', 'state', 'submitted' ] as const
type ColumnKey = typeof COLUMN_KEYS[number]

// Only the queue itself has places: the other lists are not an order slots are handed out in.
const columnKeysByList = {
  proposed: [ 'title', 'project', 'priority', 'submitted' ],
  queued: COLUMN_KEYS,
  running: [ 'title', 'project', 'priority', 'state', 'submitted' ],
  scheduled: [ 'title', 'project', 'priority', 'submitted' ],
  paused: [ 'title', 'project', 'priority', 'submitted' ],
} as const satisfies Record<WorkQueueList, readonly ColumnKey[]>

const columnLabelKeys = {
  position: 'table.columns.position',
  title: 'table.columns.title',
  project: 'table.columns.project',
  priority: 'table.columns.priority',
  state: 'table.columns.state',
  submitted: 'table.columns.submitted',
} as const satisfies Record<ColumnKey, string>

// Starting widths in pixels, summing to less than the content column at md.
const columnSizes = {
  position: 70,
  title: 260,
  project: 150,
  priority: 160,
  state: 160,
  submitted: 120,
} as const satisfies Record<ColumnKey, number>

const emptyKeys = {
  proposed: 'table.empty.proposed',
  queued: 'table.empty.queued',
  running: 'table.empty.running',
  scheduled: 'table.empty.scheduled',
  paused: 'table.empty.paused',
} as const satisfies Record<WorkQueueList, string>

// Every action a row may offer, in the order rows show them; each row shows those its batch
// allows. Rejecting and cancelling are last and red, since neither can be undone.
const ROW_ACTIONS = [
  'approve',
  'reject',
  'runNext',
  'returnToPlace',
  'resume',
  'pause',
  'makeAsap',
  'makeNormal',
  'cancel',
] as const satisfies readonly WorkBatchAction[]

// One of the work queue's lists: what each batch is, whose it is, when it should run, and where
// it stands, each title opening the batch's own page. A row's actions are those its batch allows
// now, from running it next to cancelling it.
export function WorkQueueTable(props: Props) {
  const { t } = useTranslation('workQueue')
  // A batch's priority and state read as its project's Work batches tab names them.
  const projectsTranslation = useTranslation('projects')
  const labels = useSmartTableLabels()
  const act = useWorkBatchActions()
  const now = useNow()
  useProjectsLoader()
  const projectNames = useAppSelector(selectProjectNamesById, shallowEqual)

  const managedColumns = useMemo(() => {
    const projectName = (entry: WorkQueueEntry) => projectNames[entry.projectId] ?? ''
    const priorityLabel = (entry: WorkQueueEntry) => projectsTranslation.t(batchPriorityLabelKeys[entry.priority])
    const stateLabel = (entry: WorkQueueEntry) => projectsTranslation.t(batchStateLabelKeys[entry.state])
    // A place in the queue is where it stands in the list the server ordered, counted once.
    const positions = new Map<string, number>()
    for (const [ index, entry ] of props.entries.entries()) {
      positions.set(entry.id, index + 1)
    }
    const position = (entry: WorkQueueEntry) => positions.get(entry.id) ?? 0

    const searchValue = {
      position: (entry: WorkQueueEntry) => String(position(entry)),
      title: (entry: WorkQueueEntry) => entry.title,
      project: projectName,
      priority: priorityLabel,
      state: stateLabel,
      submitted: (entry: WorkQueueEntry) => formatDateTime(entry.createdAt),
    } satisfies Record<ColumnKey, (entry: WorkQueueEntry) => string>

    // A place sorts as a number and a submission in time; the rest by their words.
    const sortValue = {
      ...searchValue,
      position,
      submitted: (entry: WorkQueueEntry) => Date.parse(entry.createdAt),
    } satisfies Record<ColumnKey, (entry: WorkQueueEntry) => string | number>

    const exportValue = {
      ...searchValue,
      position,
      submitted: (entry: WorkQueueEntry) => entry.createdAt,
    } satisfies Record<ColumnKey, (entry: WorkQueueEntry) => CsvValue>

    const renderCell = {
      position: (entry: WorkQueueEntry) => <span className='tabular-nums opacity-80'>{
        position(entry)
      }</span>,
      title: (entry: WorkQueueEntry) => <div className='flex min-w-0 flex-col items-start gap-1'>
        <Link href={getWorkBatchViewUrl(entry.id)} className='font-medium text-inherit no-underline'>{
          entry.title
        }</Link>
        {entry.pinnedAt && <Chip size='sm' variant='soft' color='accent'>{t('table.runsNext')}</Chip>}
      </div>,
      project: (entry: WorkQueueEntry) => <Link
        href={getProjectViewUrl(entry.projectId)}
        className='text-sm text-inherit no-underline'
      >{
          projectName(entry)
        }</Link>,
      // A scheduled batch names its time beneath its priority.
      priority: (entry: WorkQueueEntry) => <div className='flex flex-col items-start gap-1'>
        <Chip size='sm' variant='soft' color={batchPriorityChipColors[entry.priority]}>{
          priorityLabel(entry)
        }</Chip>
        {entry.scheduledFor && <span className='text-xs opacity-70'>{
          formatShortDateTime(entry.scheduledFor)
        }</span>}
      </div>,
      state: (entry: WorkQueueEntry) => <Chip size='sm' variant='soft' color={batchStateChipColors[entry.state]}>{
        stateLabel(entry)
      }</Chip>,
      submitted: (entry: WorkQueueEntry) => <DateText value={entry.createdAt} isFocusable={false} />,
    } satisfies Record<ColumnKey, (entry: WorkQueueEntry) => unknown>

    return createManagedColumns<WorkQueueEntry, ColumnKey>({
      columnKeys: columnKeysByList[props.list],
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
  }, [ t, projectsTranslation.t, props.list, props.entries, projectNames ])

  const rowActions = useMemo((): DataTableAction<WorkQueueEntry>[] => ROW_ACTIONS.map((action) => {
    const Icon = actionIcons[action]
    return {
      id: action,
      label: t(actionLabelKeys[action]),
      icon: <Icon className='size-4' aria-hidden />,
      isDanger: DANGEROUS_ACTIONS.includes(action),
      isVisible: (row) => availableActions(row.original, now).includes(action),
      onPress: (row) => act(row.original, action),
    }
  }), [ t, act, now ])

  if (!props.entries.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t(emptyKeys[props.list])
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: `work-queue-${props.list}-table`,
      tableLocalStorageId: `elysium.workQueue.${props.list}.table.v1`,
    }}
    tableAriaLabel={t('table.label')}
    data={props.entries}
    managedColumns={managedColumns}
    getRowId={(entry) => entry.id}
    labels={labels}
    rowActions={rowActions}
    csvExport={{ fileName: `work-queue-${props.list}` }}
    pagination={{ show: true }}
    enableSorting
    stickyHeader={false}
  />
}
