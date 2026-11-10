// Copyright © 2026 Jalapeno Labs

import type { MeetingDuplicate } from '../../api/routes/meetingRoutes'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { SWRConfig } from 'swr'

// User interface
import { SameMeetingPanel } from './SameMeetingPanel'

const routes = vi.hoisted(() => ({
  getMeetingDuplicateAskedBy: vi.fn(),
  mergeMeetingDuplicate: vi.fn(),
  keepMeetingDuplicateApart: vi.fn(),
}))

// The API is the network boundary; everything else renders for real.
vi.mock('../../api/routes/meetingRoutes', () => routes)

function pair(overrides: Partial<MeetingDuplicate> = {}): MeetingDuplicate {
  return {
    id: 'pair',
    meetingId: 'fireflies-meeting',
    meetingTitle: 'Launch review',
    meetingSourceKind: 'fireflies',
    sameAsId: 'teams-meeting',
    sameAsTitle: 'Launch review',
    sameAsSourceKind: 'teams',
    decision: null,
    decidedAt: null,
    ...overrides,
  }
}

function renderFor(itemId: string) {
  // A cache of its own per render, so no test reads another's answer.
  return render(<SWRConfig value={{ provider: () => new Map() }}>
    <MemoryRouter>
      <SameMeetingPanel itemId={itemId} />
    </MemoryRouter>
  </SWRConfig>)
}

describe('SameMeetingPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('links both meetings and merges them when asked', async () => {
    routes.getMeetingDuplicateAskedBy.mockResolvedValue({ duplicate: pair() })
    routes.mergeMeetingDuplicate.mockResolvedValue({
      duplicate: pair({ decision: 'merged', decidedAt: '2026-11-10T16:00:00Z' }),
    })
    renderFor('item')

    // Two recordings of one meeting share its title; where each came from tells them apart.
    const asked = await screen.findByText('Fireflies')
    expect(asked.closest('a')?.textContent).toBe('Launch reviewFireflies')
    expect(asked.closest('a')?.getAttribute('href')).toBe('/settings/meetings/fireflies-meeting')
    const other = screen.getByText('Teams')
    expect(other.closest('a')?.getAttribute('href')).toBe('/settings/meetings/teams-meeting')

    fireEvent.click(screen.getByText('Merge'))
    await waitFor(() => expect(routes.mergeMeetingDuplicate).toHaveBeenCalledWith('pair'))
    expect(await screen.findByText(/^Merged on /)).toBeTruthy()
    expect(screen.queryByText('Not the same')).toBeNull()
    expect(routes.keepMeetingDuplicateApart).not.toHaveBeenCalled()
  })

  it('keeps them apart when they are not the same', async () => {
    routes.getMeetingDuplicateAskedBy.mockResolvedValue({ duplicate: pair() })
    routes.keepMeetingDuplicateApart.mockResolvedValue({
      duplicate: pair({ decision: 'not_the_same', decidedAt: '2026-11-10T16:00:00Z' }),
    })
    renderFor('item')

    fireEvent.click(await screen.findByText('Not the same'))
    expect(await screen.findByText(/^Kept apart on /)).toBeTruthy()
    expect(routes.mergeMeetingDuplicate).not.toHaveBeenCalled()
  })

  it('shows nothing for an item that asks about no meetings of theirs', async () => {
    routes.getMeetingDuplicateAskedBy.mockResolvedValue({ duplicate: null })
    const { container } = renderFor('other-item')

    await waitFor(() => expect(routes.getMeetingDuplicateAskedBy).toHaveBeenCalledWith('other-item'))
    expect(container.textContent).toBe('')
  })
})
