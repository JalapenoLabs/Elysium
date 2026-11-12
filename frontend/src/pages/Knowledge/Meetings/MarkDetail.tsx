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
import { useAppSelector } from '../../../store/hooks'
import { selectProjectEntities } from '../../../store/projectsSlice'
import { selectUserEntities } from '../../../store/usersSlice'

// User interface
import { Button } from '@heroui/react'
import { LuUndo2 } from 'react-icons/lu'
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
  // A comment mark's thread, and what can be done in it; null where the meeting takes no change.
  thread: Thread | null
  threadActions: ThreadActions | null
}

// The mark selected on a meeting's transcript, open in its lane (docs/meeting-review.md,
// Clicking, selecting, and acting): what kind it is, who made it, a mark an agent made saying so,
// the words it covers, and what its kind says: how sure a confidence is and what else it might
// be, what an edit replaced with its Undo, an excerpt's projects, and a comment's thread.
export function MarkDetail(props: Props) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const users = useAppSelector(selectUserEntities)
  const projects = useAppSelector(selectProjectEntities)
  const [ isUndoing, setIsUndoing ] = useState(false)
  const { placed } = props
  const { mark } = placed
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
  // An excerpt names its projects (#241); one this person cannot see reads as unknown.
  const projectIds = Array.isArray(mark.data.projectIds)
    ? mark.data.projectIds.filter((projectId) => typeof projectId === 'string')
    : []

  return <section aria-label={t(markKindLabelKeys[placed.kind])} className='flex flex-col gap-2 text-sm'>
    <p className='font-semibold'>{t(markKindLabelKeys[placed.kind])}</p>
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

    {Boolean(projectIds.length) && <p>{
      projectIds
        .map((projectId) => projects[projectId]?.name ?? t('review.detail.unknownProject'))
        .join(', ')
    }</p>}

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

    {props.thread && <CommentThread thread={props.thread} actions={props.threadActions} />}
  </section>
}
