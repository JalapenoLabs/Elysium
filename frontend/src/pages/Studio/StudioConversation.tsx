// Copyright © 2026 Jalapeno Labs

import type { StudioFeedback, StudioItem } from '../../api/routes/studioRoutes'

// Core
import { Fragment, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectStudioItemSessions } from '../../store/codingSessionsSlice'
import { useAppSelector } from '../../store/hooks'
import { selectStudioItemFeedback } from '../../store/studioFeedbackSlice'

// User interface
import { StudioComposer } from './StudioComposer'
import { StudioSessionTimeline } from './StudioSessionTimeline'

// Misc
import { useFollowingScroll } from '../Coding/useFollowingScroll'

type Props = {
  item: StudioItem
  // A deleted item is shown read-only, without the composer.
  isReadOnly: boolean
}

// The right column: every session of the item, oldest first, read as one conversation, and
// the composer for the next prompt.
export function StudioConversation(props: Props) {
  const { t } = useTranslation('studio')
  const itemId = props.item.id
  const sessions = useAppSelector((state) => selectStudioItemSessions(state, itemId))
  const feedback = useAppSelector((state) => selectStudioItemFeedback(state, itemId))

  // Follows new messages in whichever session they arrive.
  const eventCount = useAppSelector((state) => {
    let count = 0
    for (const session of sessions) {
      count += state.sessionEvents.bySessionId[session.id]?.events.length ?? 0
    }
    return count
  })
  const { scrollRef, onScroll } = useFollowingScroll<HTMLDivElement>(eventCount)

  const feedbackByTurnId = useMemo(() => {
    const byTurnId = new Map<string, StudioFeedback>()
    for (const entry of feedback) {
      if (entry.turnId) {
        byTurnId.set(entry.turnId, entry)
      }
    }
    return byTurnId
  }, [ feedback ])

  return <section aria-label={t('conversation.label')} className='flex h-full flex-col'>
    <div
      ref={scrollRef}
      className='min-h-0 flex-1 overflow-y-auto px-4 py-4'
      onScroll={onScroll}
    >
      {!sessions.length && <p className='py-10 text-center text-sm opacity-60'>{
        t('conversation.empty')
      }</p>}

      {sessions.map((session, index) => <Fragment key={session.id}>
        {index > 0 && <div role='separator' className='my-6 h-px bg-separator' />}
        <StudioSessionTimeline
          itemId={itemId}
          session={session}
          feedbackByTurnId={feedbackByTurnId}
        />
      </Fragment>)}
    </div>

    {!props.isReadOnly && <StudioComposer
      item={props.item}
      latestSession={sessions.at(-1)}
    />}
  </section>
}
