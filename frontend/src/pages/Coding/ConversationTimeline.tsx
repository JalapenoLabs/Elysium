// Copyright © 2026 Jalapeno Labs

import type { SessionTimeline } from '../../store/sessionEventsSlice'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { TimelineEventList } from './TimelineEventList'

// Misc
import { useFollowingScroll } from './useFollowingScroll'

type Props = {
  timeline: SessionTimeline
}

// One session's conversation in its own scrolling column, following new messages.
export function ConversationTimeline(props: Props) {
  const { t } = useTranslation('coding')
  const eventCount = props.timeline.events.length
  const { scrollRef, onScroll } = useFollowingScroll<HTMLDivElement>(eventCount)

  return <div
    ref={scrollRef}
    className='h-full overflow-y-auto px-4 py-4'
    onScroll={onScroll}
  >
    {props.timeline.truncated && <p className='relaxed text-center text-xs opacity-50'>{
      t('conversation.truncated')
    }</p>}

    {!eventCount && <p className='py-10 text-center text-sm opacity-60'>{
      t('conversation.empty')
    }</p>}

    <TimelineEventList events={props.timeline.events} />
  </div>
}
