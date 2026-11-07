// Copyright © 2026 Jalapeno Labs

import type { WorkBatchWhy as Why } from '../../api/routes/workQueueRoutes'

// Core
import { render, screen } from '@testing-library/react'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { describe, expect, it } from 'vitest'

// User interface
import { WorkBatchWhy } from './WorkBatchWhy'

// Misc
import { TestAriaRouter } from '../../testRouter'

const NOTHING: Why = { records: [], messages: [], actionItems: [], parents: []}

function renderWhy(why: Why) {
  const router = createMemoryRouter([{
    path: '*',
    element: <TestAriaRouter>
      <WorkBatchWhy why={why} />
    </TestAriaRouter>,
  }])
  render(<RouterProvider router={router} />)
}

describe('WorkBatchWhy', () => {
  it('quotes the message a person sent its Project Manager, saying who sent it', () => {
    renderWhy({
      ...NOTHING,
      messages: [{
        id: 'message-1',
        authorId: 'person-1',
        author: 'Alex',
        body: 'Ship annual pricing this week.',
        sentAt: '2026-11-04T15:00:00Z',
      }],
      actionItems: [{ id: 'item-1', title: 'Annual pricing', state: 'open' }],
    })

    expect(screen.getByText('Ship annual pricing this week.')).toBeTruthy()
    expect(screen.getByText(/^Message from Alex, /)).toBeTruthy()
    expect(screen.getByRole('link', { name: 'Annual pricing' })).toBeTruthy()
  })

  it('says when its Project Manager named nothing', () => {
    renderWhy(NOTHING)

    expect(screen.getByText('Its Project Manager named nothing that called for it.')).toBeTruthy()
  })
})
