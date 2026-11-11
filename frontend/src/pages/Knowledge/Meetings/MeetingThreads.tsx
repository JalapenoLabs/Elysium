// Copyright © 2026 Jalapeno Labs

import type { MeetingWord } from '../../../api/routes/meetingRoutes'
import type { Thread } from './meetingThreads'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Chip } from '@heroui/react'
import { LuChevronDown, LuChevronRight } from 'react-icons/lu'
import { MeetingThreadCard } from './MeetingThreadCard'

// Misc
import { formatNumber } from '../../../format/numbers'

type Props = {
  words: MeetingWord[]
  open: Thread[]
  resolved: Thread[]
  selectedThreadId: string | null
  onSelectThread: (threadId: string) => void
  isLoading: boolean
  hasFailed: boolean
}

// The meeting's comment threads in its lane, below the options (docs/meeting-review.md, The
// meeting's page): the open ones in the order their ranges start, then the resolved ones, folded
// away until asked for.
export function MeetingThreads(props: Props) {
  const { t } = useTranslation('meetings')
  const [ isShowingResolved, setIsShowingResolved ] = useState(false)
  const total = props.open.length + props.resolved.length

  let notice = null
  if (props.hasFailed) {
    notice = <p className='text-sm text-danger'>{t('review.comments.loadError')}</p>
  }
  else if (!total && !props.isLoading) {
    notice = <p className='text-sm opacity-70'>{t('review.comments.empty')}</p>
  }

  const cards = (threads: Thread[]) => threads.map((thread) => <MeetingThreadCard
    key={thread.placed.mark.id}
    words={props.words}
    thread={thread}
    isSelected={thread.placed.mark.id === props.selectedThreadId}
    onSelect={() => props.onSelectThread(thread.placed.mark.id)}
  />)

  return <section aria-label={t('review.comments.title')} className='px-1'>
    <p className='compact flex items-center gap-2 text-xs font-semibold uppercase opacity-60'>
      <span>{t('review.comments.title')}</span>
      {Boolean(total) && <Chip size='sm' variant='soft'>{formatNumber(total)}</Chip>}
    </p>
    {notice}
    {Boolean(props.open.length) && <ol className='compact flex flex-col gap-2'>{cards(props.open)}</ol>}
    {Boolean(props.resolved.length) && <div>
      <Button size='sm' variant='ghost' onPress={() => setIsShowingResolved(!isShowingResolved)}>
        {isShowingResolved
          ? <LuChevronDown className='size-4' aria-hidden />
          : <LuChevronRight className='size-4' aria-hidden />}
        <span>{t('review.comments.resolvedCount', { count: props.resolved.length })}</span>
      </Button>
      {isShowingResolved && <ol className='mt-2 flex flex-col gap-2 opacity-80'>{cards(props.resolved)}</ol>}
    </div>}
  </section>
}
