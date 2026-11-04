// Copyright © 2026 Jalapeno Labs

import type { WorkBatch } from '../../api/routes/projectManagerRoutes'
import type { PersonPromise } from '../../api/routes/promiseRoutes'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { shallowEqual } from 'react-redux'

// Redux
import { selectActionItemTitlesById } from '../../store/actionItemsSlice'
import { useAppSelector } from '../../store/hooks'

// User interface
import { Chip, Link } from '@heroui/react'
import { createManagedColumns, SmartTable } from '@jalapenolabs/uikit'
import { LuExternalLink } from 'react-icons/lu'

// Misc
import { DateText } from '../../format/DateText'
import { useSmartTableLabels } from '../../hooks/useSmartTableLabels'
import { getActionItemViewUrl } from '../../urls'
import { promiseStateChipColors, promiseStateLabelKeys, promiseWork } from './projectManagerPresentation'

type Props = {
  promises: PersonPromise[]
  // The project's batches, which name the work a promise waits on.
  batches: WorkBatch[]
}

const PROMISE_COLUMN_KEYS = [ 'promise', 'recipient', 'place', 'work', 'state', 'made' ] as const
type PromiseColumnKey = typeof PROMISE_COLUMN_KEYS[number]

const columnLabelKeys = {
  promise: 'promises.columns.promise',
  recipient: 'promises.columns.recipient',
  place: 'promises.columns.place',
  work: 'promises.columns.work',
  state: 'promises.columns.state',
  made: 'promises.columns.made',
} as const satisfies Record<PromiseColumnKey, string>

// Starting widths in pixels, summing to less than the page's content column.
const columnSizes = {
  promise: 240,
  recipient: 130,
  place: 220,
  work: 200,
  state: 140,
  made: 120,
} as const satisfies Record<PromiseColumnKey, number>

// What the person promised to tell people once the project's work is done, newest first: who
// for, where they promised it, the work it waits on, and where it stands, with its follow-up once
// the Project Manager drafted one.
export function PromisesTable(props: Props) {
  const { t } = useTranslation('projects')
  const labels = useSmartTableLabels()
  const itemTitles = useAppSelector(selectActionItemTitlesById, shallowEqual)

  const managedColumns = useMemo(() => {
    function workLabel(promise: PersonPromise) {
      const work = promiseWork(promise, props.batches, itemTitles)
      if (work.kind === 'none') {
        return t('promises.noWork')
      }
      return work.title ?? t(work.kind === 'batch'
        ? 'promises.aBatch'
        : 'promises.anItem')
    }
    function stateLabel(promise: PersonPromise) {
      return t(promiseStateLabelKeys[promise.state])
    }

    const renderCell = {
      promise: (promise: PersonPromise) => <span className='font-medium'>{promise.summary}</span>,
      recipient: (promise: PersonPromise) => <span>{promise.recipient}</span>,
      place: (promise: PersonPromise) => {
        const madeIn = promise.madeIn
        if (!madeIn) {
          return <span className='text-sm opacity-60'>{t('promises.messageGone')}</span>
        }
        if (!madeIn.url) {
          return <span className='text-sm'>{madeIn.title}</span>
        }
        return <Link href={madeIn.url} target='_blank' rel='noreferrer' className='gap-1 text-sm'>
          <span>{madeIn.title}</span>
          <LuExternalLink className='size-3.5 shrink-0' aria-hidden />
        </Link>
      },
      work: (promise: PersonPromise) => {
        const work = promiseWork(promise, props.batches, itemTitles)
        if (work.itemId) {
          return <Link href={getActionItemViewUrl(work.itemId)} className='text-sm'>{workLabel(promise)}</Link>
        }
        return <span className='text-sm opacity-80'>{workLabel(promise)}</span>
      },
      // A drafted follow-up is an item of the person's, which its state links to.
      state: (promise: PersonPromise) => <div className='flex flex-col items-start gap-1'>
        <Chip size='sm' variant='soft' color={promiseStateChipColors[promise.state]}>{
          stateLabel(promise)
        }</Chip>
        {promise.followUpItemId && <Link href={getActionItemViewUrl(promise.followUpItemId)} className='text-xs'>{
          t('promises.openFollowUp')
        }</Link>}
      </div>,
      made: (promise: PersonPromise) => <DateText value={promise.createdAt} isFocusable={false} />,
    } satisfies Record<PromiseColumnKey, (promise: PersonPromise) => unknown>

    // Each sorts by its value, which is also what the export holds: the instant it was found.
    const sortValue = {
      promise: (promise: PersonPromise) => promise.summary,
      recipient: (promise: PersonPromise) => promise.recipient,
      place: (promise: PersonPromise) => promise.madeIn?.title ?? '',
      work: workLabel,
      state: stateLabel,
      made: (promise: PersonPromise) => promise.createdAt,
    } satisfies Record<PromiseColumnKey, (promise: PersonPromise) => string>

    return createManagedColumns<PersonPromise, PromiseColumnKey>({
      columnKeys: PROMISE_COLUMN_KEYS,
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
  }, [ t, props.batches, itemTitles ])

  if (!props.promises.length) {
    return <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
      t('promises.empty')
    }</p>
  }

  return <SmartTable
    ids={{
      tableElementId: 'project-promises-table',
      tableLocalStorageId: 'elysium.projects.promises.table.v1',
    }}
    tableAriaLabel={t('promises.label')}
    data={props.promises}
    managedColumns={managedColumns}
    getRowId={(promise) => promise.id}
    labels={labels}
    toolbar={{ show: false }}
    csvExport={{ fileName: 'promises' }}
    pagination={{ show: true }}
    enableSorting
    stickyHeader={false}
  />
}
