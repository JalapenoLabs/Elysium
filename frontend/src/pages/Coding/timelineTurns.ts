// Copyright © 2026 Jalapeno Labs

import type { SessionEvent, SessionEventPayload } from '../../api/routes/codingSessionRoutes'

type TurnCompleted = Extract<SessionEventPayload, { kind: 'turnCompleted' }>

// One turn of a conversation: the prompt that started it, everything the agent did, and how
// it ended. A turn without `completed` is still running, or its end was not kept.
export type TimelineTurn = {
  kind: 'turn'
  turnId: string
  prompt: SessionEvent | null
  log: SessionEvent[]
  completed: TurnCompleted | null
}

// An event that belongs to no turn, shown on its own.
export type LooseEvent = {
  kind: 'event'
  event: SessionEvent
}

export type TimelineEntry = TimelineTurn | LooseEvent

// Groups a conversation's events by turn, in one pass, keeping the order turns started in.
// The prompt is held apart so it stays visible while the turn's log is collapsed.
export function groupTimelineTurns(events: SessionEvent[]): TimelineEntry[] {
  const entries: TimelineEntry[] = []
  const turnsById = new Map<string, TimelineTurn>()

  for (const event of events) {
    if (!event.turnId) {
      entries.push({ kind: 'event', event })
      continue
    }

    let turn = turnsById.get(event.turnId)
    if (!turn) {
      turn = { kind: 'turn', turnId: event.turnId, prompt: null, log: [], completed: null }
      turnsById.set(event.turnId, turn)
      entries.push(turn)
    }

    if (event.payload?.kind === 'turnStarted' && !turn.prompt) {
      turn.prompt = event
      continue
    }
    if (event.payload?.kind === 'turnCompleted') {
      turn.completed = event.payload
    }
    turn.log.push(event)
  }

  return entries
}

// What a collapsed turn's row says when no summary was written: the first line of the agent's
// final message, if it left one.
export function getTurnFallbackLine(turn: TimelineTurn): string | null {
  const firstLine = turn.completed?.summary.trim().split('\n', 1)[0]
  return firstLine || null
}
