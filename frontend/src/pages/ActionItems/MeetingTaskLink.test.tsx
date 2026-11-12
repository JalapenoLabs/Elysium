// Copyright © 2026 Jalapeno Labs

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { SWRConfig } from 'swr'

// User interface
import { MeetingTaskLink } from './MeetingTaskLink'

// Misc
import { getTaskOfItem } from '../../api/routes/meetingRoutes'
import { TestAriaRouter } from '../../testRouter'

vi.mock('../../api/routes/meetingRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/routes/meetingRoutes')>(),
  getTaskOfItem: vi.fn(),
}))

function renderFor(itemId: string) {
  return render(<MemoryRouter>
    <SWRConfig value={{ provider: () => new Map() }}>
      <TestAriaRouter>
        <MeetingTaskLink itemId={itemId} />
      </TestAriaRouter>
    </SWRConfig>
  </MemoryRouter>)
}

describe('MeetingTaskLink', () => {
  beforeEach(() => {
    vi.mocked(getTaskOfItem).mockReset()
  })

  it('opens the meeting at the words the item\'s task was heard in', async () => {
    vi.mocked(getTaskOfItem).mockResolvedValue({
      task: { meetingId: 'sync', meetingTitle: 'Weekly sync', taskId: 'deck' },
    })
    renderFor('send-the-deck')

    const link = await screen.findByText('Open in Weekly sync')
    expect(link.closest('a')?.getAttribute('href')).toBe('/knowledge/meetings/sync?mark=deck')
    expect(getTaskOfItem).toHaveBeenCalledWith('send-the-deck')
  })

  it('names a meeting with no title as the meeting', async () => {
    vi.mocked(getTaskOfItem).mockResolvedValue({
      task: { meetingId: 'sync', meetingTitle: '  ', taskId: 'deck' },
    })
    renderFor('send-the-deck')

    expect(await screen.findByText('Open in the meeting')).toBeTruthy()
  })

  it('shows nothing for an item no task made', async () => {
    vi.mocked(getTaskOfItem).mockResolvedValue({ task: null })
    renderFor('typed-by-hand')

    await vi.waitFor(() => expect(getTaskOfItem).toHaveBeenCalled())
    expect(screen.queryByText('From a meeting')).toBeNull()
  })
})
