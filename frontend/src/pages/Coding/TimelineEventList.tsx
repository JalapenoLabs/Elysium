// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { SessionEvent } from '../../api/routes/codingSessionRoutes'
import type { TimelineTurn } from './timelineTurns'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Chip, Spinner } from '@heroui/react'
import { LuChevronRight } from 'react-icons/lu'
import { TimelineEvent } from './TimelineEvent'

// Misc
import { turnStateChipColors, turnStateLabelKeys } from './sessionPresentation'
import { getTurnFallbackLine, groupTimelineTurns } from './timelineTurns'

type Props = {
  // Sorted by sequence.
  events: SessionEvent[]
  // Each finished turn's one or two sentence summary, by turn id.
  summariesByTurnId: Readonly<Record<string, string>>
  // Anything to show under one event, such as the drawing a Studio prompt was sent with.
  renderAfterEvent?: (event: SessionEvent) => ReactNode
}

// One session's events as a conversation, read like a CI log: each prompt stays visible, and
// the work it started collapses to one row with the turn's summary. It does not scroll by
// itself, so a page can list several sessions in one scrolling column.
export function TimelineEventList(props: Props) {
  const entries = useMemo(() => groupTimelineTurns(props.events), [ props.events ])

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

  function renderEvent(event: SessionEvent) {
    return <li key={event.sequence}>
      <TimelineEvent
        event={event}
        toolNamesByCallId={toolNamesByCallId}
      />
      {props.renderAfterEvent?.(event)}
    </li>
  }

  return <ol className='mx-auto flex max-w-3xl flex-col gap-3'>{
    entries.map((entry) => {
      if (entry.kind === 'event') {
        return renderEvent(entry.event)
      }
      return <li key={entry.turnId} className='flex flex-col gap-3'>
        {entry.prompt && <ol>{renderEvent(entry.prompt)}</ol>}
        {entry.log.length > 0 && <TurnLog
          turn={entry}
          summary={props.summariesByTurnId[entry.turnId]}
        >{
          entry.log.map(renderEvent)
        }</TurnLog>}
      </li>
    })
  }</ol>
}

type TurnLogProps = {
  turn: TimelineTurn
  summary: string | undefined
  // The turn's events, already rendered.
  children: ReactNode
}

// A turn's work behind one row. A running turn stays open so its progress shows; a finished
// one closes to its summary, until the viewer opens it.
function TurnLog(props: TurnLogProps) {
  const { t } = useTranslation('coding')
  const [ chosenExpanded, setChosenExpanded ] = useState<boolean | null>(null)
  const completed = props.turn.completed
  const isExpanded = chosenExpanded ?? !completed
  const headline = props.summary
    ?? getTurnFallbackLine(props.turn)
    ?? (completed
      ? t('conversation.turnFinished', { status: t(turnStateLabelKeys[completed.status]).toLowerCase() })
      : t('conversation.turnWorking'))

  return <div className='rounded-lg border border-separator'>
    <button
      type='button'
      className='level w-full cursor-pointer gap-2 px-3 py-2 text-left text-sm hover:bg-surface-secondary'
      aria-expanded={isExpanded}
      onClick={() => setChosenExpanded(!isExpanded)}
    >
      <LuChevronRight
        className={`size-4 shrink-0 opacity-60 transition-transform ${isExpanded ? 'rotate-90' : ''}`}
        aria-hidden
      />
      <span className={`min-w-0 flex-1 ${isExpanded ? '' : 'truncate'}`}>{headline}</span>
      {completed
        ? <Chip size='sm' variant='soft' color={turnStateChipColors[completed.status]}>{
          t(turnStateLabelKeys[completed.status])
        }</Chip>
        : <Spinner size='sm' aria-label={t('conversation.turnWorking')} />}
    </button>
    {isExpanded && <ol className='flex flex-col gap-3 border-t border-separator px-3 py-3'>{
      props.children
    }</ol>}
  </div>
}
