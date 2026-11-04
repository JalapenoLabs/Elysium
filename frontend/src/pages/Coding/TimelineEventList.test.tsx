// Copyright © 2026 Jalapeno Labs

import type { ReactElement } from 'react'
import type { SessionEvent, SessionEventPayload } from '../../api/routes/codingSessionRoutes'

// Core
import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'

// Redux
import { createAppStore } from '../../store'
import { turnFocusRequested } from '../../store/sessionEventsSlice'

// User interface
import { TimelineEventList } from './TimelineEventList'

// The list reads which turn a link asked for from the store.
function renderWithStore(element: ReactElement, store = createAppStore()) {
  render(<Provider store={store}>{element}</Provider>)
  return store
}

function event(sequence: number, payload: SessionEventPayload): SessionEvent {
  return { sessionId: 1, sequence, turnId: 'turn', occurredAt: null, type: payload.kind, payload }
}

const prompt = event(1, { kind: 'turnStarted', prompt: 'Fix the login form' })
const message = event(2, { kind: 'agentMessage', text: 'Looking at the form now.' })
const finished = event(3, { kind: 'turnCompleted', status: 'completed', summary: 'Done.', error: null })

describe('TimelineEventList', () => {
  it('collapses a finished turn to its summary under the prompt, and opens it on request', () => {
    renderWithStore(<TimelineEventList
      events={[ prompt, message, finished ]}
      summariesByTurnId={{ turn: 'Fixed the login form and pushed it.' }}
    />)

    expect(screen.getByText('Fix the login form')).toBeTruthy()
    expect(screen.queryByText('Looking at the form now.')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: /Fixed the login form and pushed it\./ }))

    expect(screen.getByText('Looking at the form now.')).toBeTruthy()
  })

  it('keeps a running turn open', () => {
    renderWithStore(<TimelineEventList events={[ prompt, message ]} summariesByTurnId={{}} />)

    expect(screen.getByText('Looking at the form now.')).toBeTruthy()
  })

  it('falls back to the agent\'s final message before a summary arrives', () => {
    renderWithStore(<TimelineEventList events={[ prompt, message, finished ]} summariesByTurnId={{}} />)

    expect(screen.getByRole('button', { name: /Done\./ })).toBeTruthy()
  })

  // #193: a turn summary on a project's timeline opens its session at that turn.
  describe('a turn a link asked for', () => {
    // jsdom lays nothing out, so it has no `scrollIntoView` to spy on.
    afterEach(() => {
      Reflect.deleteProperty(Element.prototype, 'scrollIntoView')
    })

    it('opens, scrolls into view, and settles the ask', () => {
      const scrollIntoView = vi.fn()
      Element.prototype.scrollIntoView = scrollIntoView
      const store = createAppStore()
      store.dispatch(turnFocusRequested('turn'))

      renderWithStore(<TimelineEventList
        events={[ prompt, message, finished ]}
        summariesByTurnId={{ turn: 'Fixed the login form and pushed it.' }}
      />, store)

      expect(screen.getByText('Looking at the form now.')).toBeTruthy()
      expect(scrollIntoView).toHaveBeenCalledWith({ block: 'start' })
      expect(store.getState().sessionEvents.focusedTurnId).toBeNull()
      // It stays open once the ask settled.
      expect(screen.getByText('Looking at the form now.')).toBeTruthy()
    })

    it('leaves the other turns closed and the ask waiting for its own turn', () => {
      const store = createAppStore()
      store.dispatch(turnFocusRequested('another-turn'))

      renderWithStore(<TimelineEventList
        events={[ prompt, message, finished ]}
        summariesByTurnId={{}}
      />, store)

      expect(screen.queryByText('Looking at the form now.')).toBeNull()
      expect(store.getState().sessionEvents.focusedTurnId).toBe('another-turn')
    })
  })
})
