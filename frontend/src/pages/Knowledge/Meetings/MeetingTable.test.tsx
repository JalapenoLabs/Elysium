// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../../api/routes/meetingRoutes'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'

// Redux
import { createAppStore } from '../../../store'
import { meLoaded } from '../../../store/authSlice'
import { meetingsLoaded } from '../../../store/meetingsSlice'
import { projectsLoaded } from '../../../store/projectsSlice'

// User interface
import { MeetingTable } from './MeetingTable'

// Misc
import { makeMeeting, makeProject, makeUser } from '../../../testFixtures'

// Retrying, deleting, and restoring are the network boundary; toasts are watched for what they
// say.
const routes = vi.hoisted(() => ({
  retryMeeting: vi.fn(),
  changeMeetings: vi.fn(),
}))

vi.mock('../../../api/routes/meetingRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/meetingRoutes')>(),
  ...routes,
}))

const toasts = vi.hoisted(() => ({
  success: vi.fn(),
  warning: vi.fn(),
  danger: vi.fn(),
}))

vi.mock('../../../toast/toast', () => ({ toast: toasts }))

const MINUTE = 60 * 1000
const DELETED_AT = '2026-11-11T09:00:00Z'

// The reader is the person every fixture meeting belongs to, unless a meeting names another owner.
const READER_ID = makeMeeting({ id: 'any' }).ownerId

function renderTable(meetings: Meeting[], isDeletedView = false) {
  const store = createAppStore()
  store.dispatch(meLoaded({
    user: makeUser({ id: READER_ID }),
    hasSecondFactor: false,
    mfaEnrollmentRequired: false,
    authenticatorNickname: null,
  }))
  store.dispatch(meetingsLoaded(meetings))
  render(<Provider store={store}>
    <MemoryRouter>
      <MeetingTable
        meetings={meetings}
        controlsContainer={null}
        emptyNotice={<p>No meetings</p>}
        isDeletedView={isDeletedView}
      />
    </MemoryRouter>
  </Provider>)
  return store
}

// What the API answers to a change of `meetings`: each as it now stands, deleted or not.
function answerWith(meetings: Meeting[], deletedAt: string | null) {
  return { meetings: meetings.map((meeting) => ({ ...meeting, deletedAt })), refused: []}
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
    routes.changeMeetings.mockReset()
    toasts.success.mockReset()
    toasts.warning.mockReset()
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

  it('opens a meeting\'s page from its title, and links its source to its connector', () => {
    renderTable([ makeMeeting({ id: 'linked', title: 'Design review', url: 'https://app.fireflies.ai/view/1' }) ])

    const title = screen.getByText('Design review').closest('a')
    expect(title?.getAttribute('href')).toBe('/knowledge/meetings/linked')
    expect(title?.getAttribute('target')).toBeNull()
    const source = screen.getByRole('link', { name: 'Fireflies' })
    expect(source.getAttribute('href')).toBe('/settings/connectors/connector-1')
  })

  it('names the projects a meeting\'s excerpts are on, leaving out one deleted since', () => {
    const store = createAppStore()
    store.dispatch(projectsLoaded([
      makeProject({ id: 'launch', name: 'Launch' }),
      makeProject({ id: 'billing', name: 'Billing' }),
    ]))
    const meeting = makeMeeting({ id: 'planning', projectIds: [ 'launch', 'gone', 'billing' ]})
    render(<Provider store={store}>
      <MemoryRouter>
        <MeetingTable
          meetings={[ meeting ]}
          controlsContainer={null}
          emptyNotice={<p>No meetings</p>}
          isDeletedView={false}
        />
      </MemoryRouter>
    </Provider>)

    expect(screen.getByText('Launch, Billing')).toBeTruthy()
  })

  it('says under its state that a meeting\'s run waits for the person\'s approval', () => {
    renderTable([ makeMeeting({ id: 'held', state: 'recorded', awaitingApproval: true }) ])

    expect(screen.getByText('Waiting for your approval')).toBeTruthy()
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

  it('offers Retry on a failed meeting of the reader\'s own alone, and keeps what the API answers', async () => {
    const failed = makeMeeting({ id: 'failed', title: 'Failed', state: 'failed', lastError: 'Timed out' })
    routes.retryMeeting.mockResolvedValue({ meeting: { ...failed, state: 'pending', lastError: null }})
    const store = renderTable([
      failed,
      makeMeeting({ id: 'done', title: 'Done' }),
      // Someone else's processing is theirs to retry.
      makeMeeting({ id: 'theirs', title: 'Theirs', state: 'failed', ownerId: 'someone-else' }),
    ])

    const retryButtons = screen.getAllByRole('button', { name: 'Retry' })
    expect(retryButtons).toHaveLength(1)
    fireEvent.click(retryButtons[0])

    await waitFor(() => expect(store.getState().meetings.entities.failed?.state).toBe('pending'))
    expect(routes.retryMeeting).toHaveBeenCalledWith('failed')
    expect(toasts.success).toHaveBeenCalledWith('Trying this meeting again')
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

  // #258: there was no way to delete a meeting.
  it('deletes a meeting from its row at once, with Undo restoring it', async () => {
    const meeting = makeMeeting({ id: 'duplicate', title: 'Duplicate sync' })
    routes.changeMeetings.mockResolvedValueOnce(answerWith([ meeting ], DELETED_AT))
    const store = renderTable([ meeting ])

    fireEvent.click(screen.getByRole('button', { name: 'Delete' }))

    await waitFor(() => expect(store.getState().meetings.entities.duplicate?.deletedAt).toBe(DELETED_AT))
    expect(routes.changeMeetings).toHaveBeenCalledWith([ 'duplicate' ], 'delete')
    const [ message, options ] = toasts.success.mock.calls[0]
    expect(message).toBe('Deleted “Duplicate sync”.')
    expect(options.action.label).toBe('Undo')

    routes.changeMeetings.mockResolvedValueOnce(answerWith([ meeting ], null))
    options.action.onPress()
    await waitFor(() => expect(store.getState().meetings.entities.duplicate?.deletedAt).toBeNull())
    expect(routes.changeMeetings).toHaveBeenLastCalledWith([ 'duplicate' ], 'restore')
    expect(toasts.success).toHaveBeenLastCalledWith('Restored “Duplicate sync”.')
  })

  it('deletes a selection from the floating bar, naming what changed meanwhile', async () => {
    const first = makeMeeting({ id: 'first', title: 'First' })
    const second = makeMeeting({ id: 'second', title: 'Second' })
    routes.changeMeetings.mockResolvedValueOnce({
      meetings: [{ ...first, deletedAt: DELETED_AT }],
      refused: [{ meetingId: 'second', code: 'meeting_merged', message: 'merged' }],
    })
    renderTable([ first, second ])

    for (const checkbox of screen.getAllByRole('checkbox', { name: 'Select row' })) {
      fireEvent.click(checkbox)
    }
    const bar = await screen.findByRole('toolbar', { name: 'Selection' })
    fireEvent.click(within(bar).getByRole('button', { name: 'Delete' }))

    await waitFor(() => expect(routes.changeMeetings).toHaveBeenCalled())
    expect([ ...routes.changeMeetings.mock.calls[0][0] ].sort()).toEqual([ 'first', 'second' ])
    expect(routes.changeMeetings.mock.calls[0][1]).toBe('delete')
    await waitFor(() => expect(toasts.success).toHaveBeenCalled())
    expect(toasts.success.mock.calls[0][0]).toBe('Deleted “First”.')
    expect(toasts.warning).toHaveBeenCalledWith('Left “Second” as it is, since it changed meanwhile.')
  })

  it('offers Delete only on a meeting the reader may manage, and leaves the rest of a selection out', async () => {
    const own = makeMeeting({ id: 'own', title: 'Own sync' })
    const theirs = makeMeeting({ id: 'theirs', title: 'Their sync', ownerId: 'someone-else', canManage: false })
    routes.changeMeetings.mockResolvedValueOnce(answerWith([ own ], DELETED_AT))
    renderTable([ own, theirs ])

    // One Delete among the rows' actions: the reader's own meeting's.
    expect(screen.getAllByRole('button', { name: 'Delete' })).toHaveLength(1)

    for (const checkbox of screen.getAllByRole('checkbox', { name: 'Select row' })) {
      fireEvent.click(checkbox)
    }
    const bar = await screen.findByRole('toolbar', { name: 'Selection' })
    fireEvent.click(within(bar).getByRole('button', { name: 'Delete' }))

    await waitFor(() => expect(routes.changeMeetings).toHaveBeenCalledWith([ 'own' ], 'delete'))
    expect(toasts.warning).toHaveBeenCalledWith(
      'Left “Their sync” as it is, since only its owner or an admin deletes or restores it.',
    )
  })

  it('offers Restore alone on a deleted meeting', async () => {
    const deleted = makeMeeting({ id: 'gone', title: 'Gone', state: 'failed', deletedAt: DELETED_AT })
    routes.changeMeetings.mockResolvedValueOnce(answerWith([ deleted ], null))
    const store = renderTable([ deleted ], true)

    expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Delete' })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Restore' }))

    await waitFor(() => expect(store.getState().meetings.entities.gone?.deletedAt).toBeNull())
    expect(routes.changeMeetings).toHaveBeenCalledWith([ 'gone' ], 'restore')
  })

  it('reads a deleted meeting as deleted, not as on the timeline, keeping why it failed', () => {
    renderTable([
      makeMeeting({ id: 'finished', title: 'Finished', state: 'done', deletedAt: DELETED_AT }),
      makeMeeting({
        id: 'broken',
        title: 'Broken',
        state: 'failed',
        lastError: 'Fireflies refused the key',
        deletedAt: DELETED_AT,
      }),
    ], true)

    expect(screen.getAllByText('Deleted')).toHaveLength(2)
    expect(screen.queryByText('On the timeline')).toBeNull()
    expect(screen.getByText('Fireflies refused the key')).toBeTruthy()
  })
})
