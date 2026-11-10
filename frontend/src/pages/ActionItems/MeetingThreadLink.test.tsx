// Copyright © 2026 Jalapeno Labs

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { SWRConfig } from 'swr'

// User interface
import { MeetingThreadLink } from './MeetingThreadLink'

// Misc
import { getThreadOfItem } from '../../api/routes/meetingRoutes'
import { TestAriaRouter } from '../../testRouter'

vi.mock('../../api/routes/meetingRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/routes/meetingRoutes')>(),
  getThreadOfItem: vi.fn(),
}))

function renderFor(itemId: string) {
  return render(<MemoryRouter>
    <SWRConfig value={{ provider: () => new Map() }}>
      <TestAriaRouter>
        <MeetingThreadLink itemId={itemId} />
      </TestAriaRouter>
    </SWRConfig>
  </MemoryRouter>)
}

describe('MeetingThreadLink', () => {
  beforeEach(() => {
    vi.mocked(getThreadOfItem).mockReset()
  })

  it('opens the meeting at the thread the item was raised by', async () => {
    vi.mocked(getThreadOfItem).mockResolvedValue({ thread: { meetingId: 'sync', threadId: 'note' }})
    renderFor('mention')

    const link = await screen.findByText('Open the thread')
    expect(link.closest('a')?.getAttribute('href')).toBe('/knowledge/meetings/sync?thread=note')
    expect(getThreadOfItem).toHaveBeenCalledWith('mention')
  })

  it('shows nothing for an item no thread raised', async () => {
    vi.mocked(getThreadOfItem).mockResolvedValue({ thread: null })
    renderFor('other')

    await vi.waitFor(() => expect(getThreadOfItem).toHaveBeenCalled())
    expect(screen.queryByText('Comment thread')).toBeNull()
  })
})
