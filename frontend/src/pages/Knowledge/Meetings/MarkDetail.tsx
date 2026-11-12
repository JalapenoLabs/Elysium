// Copyright © 2026 Jalapeno Labs

import type { MeetingWord } from '../../../api/routes/meetingRoutes'
import type { ThreadActions } from './CommentThread'
import type { HistoryRow } from './meetingReviewPresentation'
import type { PlacedMark } from './meetingTranscript'
import type { Thread } from './meetingThreads'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useTimelineRecordLoader } from '../../../hooks/useServerData'
import { useAppSelector } from '../../../store/hooks'
import { selectTimelineRecordById } from '../../../store/timelineRecordsSlice'
import { selectUserEntities } from '../../../store/usersSlice'

// User interface
import { Button, Spinner } from '@heroui/react'
import { LuEye, LuTrash2, LuUndo2 } from 'react-icons/lu'
import { RecordProjects } from '../../Projects/RecordProjects'
import { CommentThread } from './CommentThread'

// Misc
import { DateText } from '../../../format/DateText'
import { formatMediaPosition } from '../../../format/durations'
import { formatNumber } from '../../../format/numbers'
import { authorOf, entryOfEditMark, markKindLabelKeys } from './meetingReviewPresentation'
import { alternativesOf, confidenceOf, summarizeRange, textOfRange } from './meetingTranscript'

type Props = {
  words: MeetingWord[]
  placed: PlacedMark
  // The meeting's history, newest first, which says what an edit replaced.
  history: HistoryRow[]
  onUndo: (entryId: string) => Promise<void>
  // Removes an excerpt, or a black-out, which its toast's Undo puts back; null where the meeting
  // takes no change.
  onRemove: ((markId: string, kind: 'excerpt' | 'black_out') => Promise<void>) | null
  // A comment mark's thread, and what can be done in it; null where the meeting takes no change.
  thread: Thread | null
  threadActions: ThreadActions | null
}

// The mark selected on a meeting's transcript, open in its lane (docs/meeting-review.md,
// Clicking, selecting, and acting): what kind it is, who made it, a mark an agent made saying so,
// the words it covers, and what its kind says: how sure a confidence is and what else it might
// be, what an edit replaced with its Undo, an excerpt's title and projects, which change on its
// record as any record's do, with Remove, a black-out's Remove, and a comment's thread.
export function MarkDetail(props: Props) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const users = useAppSelector(selectUserEntities)
  const [ isUndoing, setIsUndoing ] = useState(false)
  const [ isRemoving, setIsRemoving ] = useState(false)
  const { placed, onRemove } = props
  const { mark } = placed
  // An excerpt's record, read as it opens, which its projects show and change.
  const recordStatus = useTimelineRecordLoader(mark.excerpt?.recordId ?? null)
  const record = useAppSelector((state) => mark.excerpt
    ? selectTimelineRecordById(state, mark.excerpt.recordId)
    : undefined)
  const author = authorOf(mark.createdBy, mark.agent, users)
  const span = summarizeRange(props.words, placed)

  let byline = t('review.detail.byPerson', { name: author.name ?? t('common:table.someone') })
  if (author.isAgent) {
    byline = author.name
      ? t('review.detail.byAgent', { name: author.name })
      : t('review.detail.byUnnamedAgent')
  }

  const editEntry = placed.kind === 'edit'
    ? entryOfEditMark(props.history.map((row) => row.entry), mark.id)
    : null
  const editRow = editEntry
    ? props.history.find((row) => row.entry.id === editEntry.id) ?? null
    : null
  const confidence = placed.kind === 'confidence'
    ? confidenceOf(mark)
    : null
  const alternatives = placed.kind === 'confidence'
    ? alternativesOf(mark)
    : []
  const removable = placed.kind === 'excerpt' || placed.kind === 'black_out'
    ? placed.kind
    : null

  return <section aria-label={t(markKindLabelKeys[placed.kind])} className='flex flex-col gap-2 text-sm'>
    <p className='font-semibold'>{t(markKindLabelKeys[placed.kind])}</p>
    {mark.excerpt && <p className='font-medium'>{mark.excerpt.title}</p>}
    <p className='text-xs opacity-70'>
      <span>{byline}</span>
      <span aria-hidden> · </span>
      <DateText value={mark.createdAt} />
    </p>
    <p className='text-xs opacity-70'>{
      t('review.detail.span', {
        start: formatMediaPosition(span.startMs),
        end: formatMediaPosition(span.endMs),
      })
    }</p>
    <blockquote className='border-l-2 border-separator pl-2 opacity-90'>{textOfRange(props.words, placed)}</blockquote>

    {confidence !== null && <p>{t('review.detail.confidence', { confidence: formatNumber(confidence) })}</p>}
    {Boolean(alternatives.length) && <div>
      <p className='text-xs opacity-70'>{t('review.detail.alternatives')}</p>
      <ul className='list-inside list-disc'>{
        alternatives.map((alternative) => <li key={alternative}>{alternative}</li>)
      }</ul>
    </div>}

    {record && <RecordProjects record={record} />}
    {!record && recordStatus === 'loading' && <Spinner size='sm' aria-label={t('review.excerpts.loading')} />}
    {!record && recordStatus === 'failed' && <p className='text-xs text-danger'>
      {t('review.excerpts.loadError')}
    </p>}
    {placed.kind === 'black_out' && <p className='text-xs opacity-70'>{t('review.blackOuts.explained')}</p>}

    {editRow?.before && <p className='text-xs opacity-70'>{t('review.detail.replaced', { before: editRow.before })}</p>}
    {editRow?.canUndo && <Button
      size='sm'
      variant='outline'
      className='self-start'
      isPending={isUndoing}
      onPress={() => {
        setIsUndoing(true)
        void props.onUndo(editRow.entry.id).finally(() => setIsUndoing(false))
      }}
    >
      <LuUndo2 className='size-4' aria-hidden />
      <span>{t('review.undoEdit')}</span>
    </Button>}

    {removable && onRemove && <Button
      size='sm'
      variant='outline'
      className='self-start'
      isPending={isRemoving}
      onPress={() => {
        setIsRemoving(true)
        void onRemove(mark.id, removable).finally(() => setIsRemoving(false))
      }}
    >
      {removable === 'excerpt'
        ? <LuTrash2 className='size-4' aria-hidden />
        : <LuEye className='size-4' aria-hidden />}
      <span>{
        removable === 'excerpt'
          ? t('review.excerpts.remove')
          : t('review.blackOuts.remove')
      }</span>
    </Button>}

    {props.thread && <CommentThread thread={props.thread} actions={props.threadActions} />}
  </section>
}
