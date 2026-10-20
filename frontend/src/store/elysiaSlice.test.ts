// Copyright © 2026 Jalapeno Labs

import type { ElysiaMessage, ElysiaTurn } from '../api/routes/elysiaRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import {
  elysiaMessageCreated,
  elysiaMessagesLoaded,
  elysiaPanelToggled,
  elysiaTurnsLoaded,
  elysiaTurnUpserted,
  selectElysiaMessages,
  selectElysiaTurns,
  selectIsElysiaPanelOpen,
} from './elysiaSlice'

// UUIDv7-like ids, which sort the way messages were sent.
const FIRST = '01920000-0000-7000-8000-000000000001'
const SECOND = '01920000-0000-7000-8000-000000000002'
const THIRD = '01920000-0000-7000-8000-000000000003'

function message(id: string, fromElysia = false): ElysiaMessage {
  return {
    id,
    body: `message ${id}`,
    fromElysia,
    createdBy: '00000000-0000-0000-0000-000000000002',
    createdAt: '2026-10-20T15:00:00Z',
  }
}

function turn(id: string, status: ElysiaTurn['status']): ElysiaTurn {
  return {
    id,
    triggerKind: 'message',
    triggerDetail: {},
    status,
    summary: null,
    error: null,
    createdBy: '00000000-0000-0000-0000-000000000001',
    createdAt: '2026-10-20T15:00:00Z',
    startedAt: null,
    finishedAt: null,
  }
}

describe('elysiaSlice', () => {
  it('keeps the conversation oldest first, merging pages with live messages', () => {
    const store = createAppStore()
    store.dispatch(elysiaMessageCreated(message(THIRD, true)))
    store.dispatch(elysiaMessagesLoaded([ message(SECOND), message(FIRST) ]))

    const ids = selectElysiaMessages(store.getState()).map((entry) => entry.id)
    expect(ids).toEqual([ FIRST, SECOND, THIRD ])
  })

  it('lists turns newest first and updates one in place', () => {
    const store = createAppStore()
    store.dispatch(elysiaTurnsLoaded([ turn(FIRST, 'succeeded'), turn(SECOND, 'queued') ]))
    store.dispatch(elysiaTurnUpserted(turn(SECOND, 'running')))

    const turns = selectElysiaTurns(store.getState())
    expect(turns.map((entry) => entry.id)).toEqual([ SECOND, FIRST ])
    expect(turns[0].status).toBe('running')
  })

  it('toggles the panel, or sets it when told which way', () => {
    const store = createAppStore()
    const startedOpen = selectIsElysiaPanelOpen(store.getState())

    store.dispatch(elysiaPanelToggled())
    expect(selectIsElysiaPanelOpen(store.getState())).toBe(!startedOpen)

    store.dispatch(elysiaPanelToggled(false))
    expect(selectIsElysiaPanelOpen(store.getState())).toBe(false)
  })
})
