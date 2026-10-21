// Copyright © 2026 Jalapeno Labs

import type { SessionEvent, SessionEventPayload } from '../../api/routes/codingSessionRoutes'

// Core
import { describe, expect, it } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'

// User interface
import { TimelineEventList } from './TimelineEventList'

function event(sequence: number, payload: SessionEventPayload): SessionEvent {
  return { sessionId: 1, sequence, turnId: 'turn', occurredAt: null, type: payload.kind, payload }
}

const prompt = event(1, { kind: 'turnStarted', prompt: 'Fix the login form' })
const message = event(2, { kind: 'agentMessage', text: 'Looking at the form now.' })
const finished = event(3, { kind: 'turnCompleted', status: 'completed', summary: 'Done.', error: null })

describe('TimelineEventList', () => {
  it('collapses a finished turn to its summary under the prompt, and opens it on request', () => {
    render(<TimelineEventList
      events={[ prompt, message, finished ]}
      summariesByTurnId={{ turn: 'Fixed the login form and pushed it.' }}
    />)

    expect(screen.getByText('Fix the login form')).toBeTruthy()
    expect(screen.queryByText('Looking at the form now.')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: /Fixed the login form and pushed it\./ }))

    expect(screen.getByText('Looking at the form now.')).toBeTruthy()
  })

  it('keeps a running turn open', () => {
    render(<TimelineEventList events={[ prompt, message ]} summariesByTurnId={{}} />)

    expect(screen.getByText('Looking at the form now.')).toBeTruthy()
  })

  it('falls back to the agent\'s final message before a summary arrives', () => {
    render(<TimelineEventList events={[ prompt, message, finished ]} summariesByTurnId={{}} />)

    expect(screen.getByRole('button', { name: /Done\./ })).toBeTruthy()
  })
})
