// Copyright © 2026 Jalapeno Labs

import type { HistoryRow } from './meetingReviewPresentation'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectUserEntities } from '../../../store/usersSlice'

// User interface
import { Button, Spinner } from '@heroui/react'
import { LuUndo2 } from 'react-icons/lu'

// Misc
import { DateText } from '../../../format/DateText'
import { usePendingAction } from '../../../hooks/usePendingAction'
import { authorOf, historySentenceKeys, markKindLabelKeys } from './meetingReviewPresentation'

// How many changes the lane lists before it offers the rest, so a meeting edited a thousand times
// still opens at once.
const SHOWN_AT_FIRST = 20

type Props = {
  rows: HistoryRow[]
  isLoading: boolean
  hasFailed: boolean
  isShowingAll: boolean
  onShowAll: () => void
  onUndo: (entryId: string) => Promise<void>
}

// Every change to a meeting, newest first, who made it, and Undo on each that still can be
// (docs/meetings.md, History). It sits in the lane below the options and the selected mark.
export function MeetingHistory(props: Props) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const users = useAppSelector(selectUserEntities)
  const pending = usePendingAction()
  const shown = props.isShowingAll
    ? props.rows
    : props.rows.slice(0, SHOWN_AT_FIRST)

  if (props.hasFailed) {
    return <p className='text-sm text-danger'>{t('review.history.loadError')}</p>
  }
  if (props.isLoading) {
    return <div className='grid place-items-center py-4'><Spinner size='sm' /></div>
  }
  if (!props.rows.length) {
    return <p className='text-sm opacity-70'>{t('review.history.empty')}</p>
  }

  return <div>
    <ol className='flex flex-col gap-3'>{
      shown.map((row) => {
        const author = authorOf(row.entry.createdBy, row.entry.agent, users)
        const name = author.name ?? (author.isAgent
          ? t('review.detail.unnamedAgent')
          : t('common:table.someone'))
        return <li key={row.entry.id} className='flex items-start gap-2 text-sm'>
          <div className='min-w-0 flex-1'>
            <p className='break-words'>{
              t(historySentenceKeys[row.sentence], {
                before: row.before ?? '',
                after: row.after ?? '',
                kind: row.markKind
                  ? t(markKindLabelKeys[row.markKind])
                  : t('review.history.unknownKind'),
              })
            }</p>
            <p className='text-xs opacity-60'>
              <span>{name}</span>
              <span aria-hidden> · </span>
              <DateText value={row.entry.createdAt} />
            </p>
          </div>
          {row.canUndo && <Button
            size='sm'
            variant='ghost'
            isPending={pending.pendingKey === row.entry.id}
            isDisabled={pending.isOtherPending(row.entry.id)}
            onPress={() => void pending.run(row.entry.id, () => props.onUndo(row.entry.id))}
          >
            <LuUndo2 className='size-4' aria-hidden />
            <span>{t('review.undo')}</span>
          </Button>}
        </li>
      })
    }</ol>
    {shown.length < props.rows.length && <Button size='sm' variant='ghost' className='mt-2' onPress={props.onShowAll}>
      <span>{t('review.history.showAll', { count: props.rows.length })}</span>
    </Button>}
  </div>
}
