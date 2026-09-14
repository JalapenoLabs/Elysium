// Copyright © 2026 Jalapeno Labs

import type { SessionTimeline } from '../../store/sessionEventsSlice'

// Core
import { useLayoutEffect, useMemo, useRef } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { TimelineEvent } from './TimelineEvent'

type Props = {
  timeline: SessionTimeline
}

// How close to the bottom, in pixels, still counts as "following" the conversation.
const FOLLOW_THRESHOLD_PX = 48

export function ConversationTimeline(props: Props) {
  const { t } = useTranslation('coding')
  const scrollRef = useRef<HTMLDivElement>(null)
  // Starts true so opening a conversation lands on its latest message.
  const isFollowingRef = useRef(true)

  const eventCount = props.timeline.events.length

  // Some harnesses name the tool only when it starts; completions look the name up here.
  const toolNamesByCallId = useMemo(() => {
    const names = new Map<string, string>()
    for (const event of props.timeline.events) {
      if (event.payload?.kind === 'toolStarted' && event.payload.toolName) {
        names.set(event.payload.toolCallId, event.payload.toolName)
      }
    }
    return names
  }, [ props.timeline.events ])

  useLayoutEffect(() => {
    const container = scrollRef.current
    if (container && isFollowingRef.current) {
      container.scrollTop = container.scrollHeight
    }
  }, [ eventCount ])

  return <div
    ref={scrollRef}
    className='h-full overflow-y-auto px-4 py-4'
    onScroll={(event) => {
      const container = event.currentTarget
      const distanceFromBottom = container.scrollHeight - container.scrollTop - container.clientHeight
      // Scrolling up to read stops auto-scroll; returning to the bottom resumes it.
      isFollowingRef.current = distanceFromBottom <= FOLLOW_THRESHOLD_PX
    }}
  >
    {props.timeline.truncated && <p className='relaxed text-center text-xs opacity-50'>{
      t('conversation.truncated')
    }</p>}

    {!eventCount && <p className='py-10 text-center text-sm opacity-60'>{
      t('conversation.empty')
    }</p>}

    <ol className='mx-auto flex max-w-3xl flex-col gap-3'>{
      props.timeline.events.map((event) => <li key={event.sequence}>
        <TimelineEvent
          event={event}
          toolNamesByCallId={toolNamesByCallId}
        />
      </li>)
    }</ol>
  </div>
}
