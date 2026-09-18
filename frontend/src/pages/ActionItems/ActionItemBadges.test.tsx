// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'

// User interface
import { ActionItemBadges } from './ActionItemBadges'

// Misc
import { makeActionItem } from '../../testFixtures'

const NOW = Date.parse('2026-09-18T12:00:00.000Z')

describe('ActionItemBadges', () => {
  it('shows nothing extra for a plain open item of the user’s', () => {
    const { container } = render(<ActionItemBadges item={makeActionItem({ id: 'plain' })} now={NOW} />)

    expect(container.textContent).toBe('')
  })

  it('marks an item overdue once its due moment has passed', () => {
    const item = makeActionItem({ id: 'late', dueAt: '2026-09-17T12:00:00.000Z' })
    render(<ActionItemBadges item={item} now={NOW} />)

    expect(screen.getByText(/^Overdue since/)).toBeTruthy()
  })

  it('shows priority, waiting, a snooze that still holds, and another owner', () => {
    const item = makeActionItem({
      id: 'busy',
      state: 'inbox',
      priority: 'urgent',
      waitingOn: 'Sam',
      snoozedUntil: '2026-09-19T12:00:00.000Z',
      owner: { kind: 'other', name: 'Alex' },
    })
    render(<ActionItemBadges item={item} now={NOW} showState />)

    expect(screen.getByText('Inbox')).toBeTruthy()
    expect(screen.getByText('Urgent')).toBeTruthy()
    expect(screen.getByText('Waiting on Sam')).toBeTruthy()
    expect(screen.getByText(/^Snoozed until/)).toBeTruthy()
    expect(screen.getByText('Owned by Alex')).toBeTruthy()
  })

  it('drops a snooze that has run out', () => {
    const item = makeActionItem({ id: 'awake', snoozedUntil: '2026-09-17T12:00:00.000Z' })
    render(<ActionItemBadges item={item} now={NOW} />)

    expect(screen.queryByText(/^Snoozed until/)).toBeNull()
  })
})
