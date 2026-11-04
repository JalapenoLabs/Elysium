// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../../api/routes/meetingRoutes'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'

// Redux
import { createAppStore } from '../../../store'
import { meetingsLoaded } from '../../../store/meetingsSlice'

// User interface
import { MeetingTable } from './MeetingTable'

// Misc
import { makeMeeting } from '../../../testFixtures'

// Retrying is the network boundary; toasts are watched for what they say.
const routes = vi.hoisted(() => ({
  retryMeeting: vi.fn(),
}))

vi.mock('../../../api/routes/meetingRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/meetingRoutes')>(),
  ...routes,
}))

const toasts = vi.hoisted(() => ({
  success: vi.fn(),
  danger: vi.fn(),
}))

vi.mock('../../../toast/toast', () => ({ toast: toasts }))

const MINUTE = 60 * 1000

function renderTable(meetings: Meeting[]) {
  const store = createAppStore()
  store.dispatch(meetingsLoaded(meetings))
  render(<Provider store={store}>
    <MemoryRouter>
      <MeetingTable meetings={meetings} controlsContainer={null} emptyNotice={<p>No meetings</p>} />
    </MemoryRouter>
  </Provider>)
  return store
}

// Presses a column's header until it sorts ascending: number columns start descending.
function sortAscending(columnLabel: string) {
  const button = screen.getByText(columnLabel).closest('button')
  if (!button) {
    throw new Error(`The ${columnLabel} column cannot be sorted`)
  }
  for (let press = 0; press < 3 && button.getAttribute('aria-label') !== 'Sorted ascending'; press++) {
    fireEvent.click(button)
  }
  expect(button.getAttribute('aria-label')).toBe('Sorted ascending')
}

// The rows' titles, top to bottom.
function rowOrder(titles: string[]) {
  return screen
    .getAllByText(new RegExp(`^(${titles.join('|')})$`))
    .map((element) => element.textContent)
}

describe('MeetingTable', () => {
  beforeEach(() => {
    routes.retryMeeting.mockReset()
    toasts.success.mockReset()
    toasts.danger.mockReset()
  })

  it('sorts by how long each meeting ran, one whose length is unknown last', () => {
    // As text, "1h 5m" sorts before "9m".
    renderTable([
      makeMeeting({ id: 'long', title: 'Long', durationMs: 65 * MINUTE }),
      makeMeeting({ id: 'unknown', title: 'Unknown', durationMs: null }),
      makeMeeting({ id: 'short', title: 'Short', durationMs: 9 * MINUTE }),
    ])

    sortAscending('Duration')
    expect(rowOrder([ 'Long', 'Unknown', 'Short' ])).toEqual([ 'Short', 'Long', 'Unknown' ])
  })

  it('opens a meeting at its source in a new tab, and links its source to its connector', () => {
    renderTable([ makeMeeting({ id: 'linked', title: 'Design review', url: 'https://app.fireflies.ai/view/1' }) ])

    const title = screen.getByText('Design review').closest('a')
    expect(title?.getAttribute('href')).toBe('https://app.fireflies.ai/view/1')
    expect(title?.getAttribute('target')).toBe('_blank')
    const source = screen.getByRole('link', { name: 'Fireflies' })
    expect(source.getAttribute('href')).toBe('/settings/connectors/connector-1')
  })

  it('names a meeting whose connector is gone by its source, without a link', () => {
    renderTable([ makeMeeting({ id: 'orphan', connectorId: null }) ])

    expect(screen.getByText('Fireflies')).toBeTruthy()
    expect(screen.queryByRole('link', { name: 'Fireflies' })).toBeNull()
  })

  it('says under its state why a meeting failed, and that a finished one had no recording', () => {
    renderTable([
      makeMeeting({ id: 'failed', state: 'failed', lastError: 'Fireflies answered 401' }),
      makeMeeting({ id: 'transcript', state: 'done', hasRecording: false }),
    ])

    expect(screen.getByText('Fireflies answered 401')).toBeTruthy()
    expect(screen.getByText('Transcript only')).toBeTruthy()
  })

  it('offers Retry on a failed meeting alone, and keeps what the API answers', async () => {
    const failed = makeMeeting({ id: 'failed', title: 'Failed', state: 'failed', lastError: 'Timed out' })
    routes.retryMeeting.mockResolvedValue({ meeting: { ...failed, state: 'pending', lastError: null }})
    const store = renderTable([ failed, makeMeeting({ id: 'done', title: 'Done' }) ])

    const retryButtons = screen.getAllByRole('button', { name: 'Retry' })
    expect(retryButtons).toHaveLength(1)
    fireEvent.click(retryButtons[0])

    await waitFor(() => expect(store.getState().meetings.entities.failed?.state).toBe('pending'))
    expect(routes.retryMeeting).toHaveBeenCalledWith('failed')
    expect(toasts.success).toHaveBeenCalledWith('Elysium will try this meeting again within a minute.')
  })

  it('says why a retry was refused', async () => {
    routes.retryMeeting.mockRejectedValue(new Error('offline'))
    renderTable([ makeMeeting({ id: 'failed', state: 'failed' }) ])

    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))

    await waitFor(() => expect(toasts.danger).toHaveBeenCalled())
  })

  it('shows its empty notice in place of the rows when there are no meetings', () => {
    renderTable([])

    expect(screen.getByText('No meetings')).toBeTruthy()
  })
})
