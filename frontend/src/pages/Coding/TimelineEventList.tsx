// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { SessionEvent } from '../../api/routes/codingSessionRoutes'

// Core
import { useMemo } from 'react'

// User interface
import { TimelineEvent } from './TimelineEvent'

type Props = {
  // Sorted by sequence.
  events: SessionEvent[]
  // Anything to show under one event, such as the drawing a Studio prompt was sent with.
  renderAfterEvent?: (event: SessionEvent) => ReactNode
}

// One session's events as a conversation. It does not scroll by itself, so a page can list
// several sessions in one scrolling column.
export function TimelineEventList(props: Props) {
  // Some harnesses name the tool only when it starts; completions look the name up here.
  const toolNamesByCallId = useMemo(() => {
    const names = new Map<string, string>()
    for (const event of props.events) {
      if (event.payload?.kind === 'toolStarted' && event.payload.toolName) {
        names.set(event.payload.toolCallId, event.payload.toolName)
      }
    }
    return names
  }, [ props.events ])

  return <ol className='mx-auto flex max-w-3xl flex-col gap-3'>{
    props.events.map((event) => <li key={event.sequence}>
      <TimelineEvent
        event={event}
        toolNamesByCallId={toolNamesByCallId}
      />
      {props.renderAfterEvent?.(event)}
    </li>)
  }</ol>
}
