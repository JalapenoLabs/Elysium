// Copyright © 2026 Jalapeno Labs

import type { MeetingWord } from '../../../api/routes/meetingRoutes'
import type { Thread } from './meetingThreads'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectUserEntities } from '../../../store/usersSlice'

// Misc
import { authorOf } from './meetingReviewPresentation'
import { textOfRange } from './meetingTranscript'

type Props = {
  words: MeetingWord[]
  thread: Thread
  isSelected: boolean
  onSelect: () => void
}

// One thread in a meeting's list of them: the words it is on, and its first comment with how
// many replies follow. Selecting it selects its range, which opens the whole thread in the lane.
export function MeetingThreadCard(props: Props) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const users = useAppSelector(selectUserEntities)
  const [ first ] = props.thread.comments
  const replies = Math.max(props.thread.comments.length - 1, 0)
  const author = first
    ? authorOf(first.createdBy, first.agent, users).name
    : null

  return <li>
    <button
      type='button'
      aria-current={props.isSelected}
      className={[
        'w-full rounded-xl border p-3 text-left text-sm transition-colors',
        props.isSelected
          ? 'border-accent bg-surface'
          : 'border-border hover:bg-surface',
      ].join(' ')}
      onClick={props.onSelect}
    >
      <span className='mb-1 block truncate text-xs opacity-70'>{
        textOfRange(props.words, props.thread.placed)
      }</span>
      {first && <span className='line-clamp-2'>
        <span className='font-semibold'>{author ?? t('common:table.someone')}</span>
        <span>: {first.body}</span>
      </span>}
      {Boolean(replies) && <span className='mt-1 block text-xs opacity-70'>{
        t('review.comments.replies', { count: replies })
      }</span>}
    </button>
  </li>
}
