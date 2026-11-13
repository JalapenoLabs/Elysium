// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../../api/routes/meetingRoutes'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'

// Redux
import { createAppStore } from '../../../store'
import { meetingsLoaded } from '../../../store/meetingsSlice'
import { usersLoaded } from '../../../store/usersSlice'

// User interface
import { MeetingVisibility } from './MeetingVisibility'

// Misc
import { makeMeeting, makeUser } from '../../../testFixtures'

// Changing who reads it is the network boundary; toasts are watched for what they say.
const routes = vi.hoisted(() => ({
  setMeetingVisibility: vi.fn(),
  setMeetingShared: vi.fn(),
  undoMeetingChange: vi.fn(),
}))

vi.mock('../../../api/routes/meetingRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/meetingRoutes')>(),
  ...routes,
}))

vi.mock('../../../hooks/useServerData', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../hooks/useServerData')>(),
  useUsersLoader: () => 'loaded',
}))

const toasts = vi.hoisted(() => ({
  success: vi.fn(),
  warning: vi.fn(),
  danger: vi.fn(),
}))

vi.mock('../../../toast/toast', () => ({ toast: toasts }))

const ADA = makeUser({ id: 'ada', name: 'Ada Lovelace' })
const GRACE = makeUser({ id: 'grace', name: 'Grace Hopper', email: 'grace@example.com' })

function renderVisibility(meeting: Meeting) {
  const store = createAppStore()
  store.dispatch(usersLoaded([ ADA, GRACE ]))
  store.dispatch(meetingsLoaded([ meeting ]))
  render(<Provider store={store}>
    <MeetingVisibility meeting={meeting} />
  </Provider>)
  return store
}

describe('MeetingVisibility', () => {
  beforeEach(() => {
    for (const mock of [ ...Object.values(routes), ...Object.values(toasts) ]) {
      mock.mockReset()
    }
  })

  it('says why its correction kept it private, and whom it is shared with', () => {
    renderVisibility(makeMeeting({
      id: 'review',
      ownerId: 'ada',
      visibility: 'private',
      sharedWith: [ 'grace' ],
      canManage: false,
      visibilityChange: {
        id: 'decided',
        kind: 'visibility_changed',
        createdBy: 'system',
        agent: null,
        data: { before: 'unchecked', after: 'private', correction: true, why: 'Grace\'s review.' },
        undid: null,
        createdAt: '2026-11-11T09:00:00Z',
      },
    }))

    expect(screen.getByText('Private')).toBeTruthy()
    expect(screen.getByText('Ada Lovelace’s meeting')).toBeTruthy()
    expect(screen.getByText('Elysia kept it private: Grace\'s review.')).toBeTruthy()
    expect(screen.getByText('Grace Hopper')).toBeTruthy()
    // Someone who may not manage it reads all of that and changes none of it.
    expect(screen.queryByRole('button', { name: 'Open to everyone' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Stop sharing with Grace Hopper' })).toBeNull()
  })

  it('offers its owner no change to who reads a deleted meeting until it is restored', () => {
    renderVisibility(makeMeeting({
      id: 'review',
      ownerId: 'ada',
      visibility: 'private',
      sharedWith: [ 'grace' ],
      canManage: true,
      deletedAt: '2026-11-11T09:00:00Z',
    }))

    expect(screen.getByText('Private')).toBeTruthy()
    expect(screen.getByText('Grace Hopper')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Open to everyone' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Stop sharing with Grace Hopper' })).toBeNull()
  })

  it('opens a private meeting at once, with Undo in its toast', async () => {
    const meeting = makeMeeting({ id: 'review', ownerId: 'ada', visibility: 'unchecked', canManage: true })
    const opened = { ...meeting, visibility: 'public' as const }
    routes.setMeetingVisibility.mockResolvedValue({ meeting: opened, entry: { id: 'opened' }})
    routes.undoMeetingChange.mockResolvedValue({})
    const store = renderVisibility(meeting)

    expect(screen.getByText('Private until checked')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Open to everyone' }))

    await waitFor(() => expect(store.getState().meetings.entities.review?.visibility).toBe('public'))
    expect(routes.setMeetingVisibility).toHaveBeenCalledWith('review', 'public')
    const [ confirmation, options ] = toasts.success.mock.calls[0]
    expect(confirmation).toBe('Meeting opened to everyone')
    options.action.onPress()
    await waitFor(() => expect(routes.undoMeetingChange).toHaveBeenCalledWith('review', 'opened'))
  })

  it('stops sharing with someone at once, and says when someone did it first', async () => {
    const meeting = makeMeeting({
      id: 'review',
      ownerId: 'ada',
      visibility: 'private',
      sharedWith: [ 'grace' ],
      canManage: true,
    })
    routes.setMeetingShared.mockRejectedValue(Object.assign(new Error('conflict'), {
      response: { status: 409 },
      data: { code: 'meeting_visibility_unchanged', message: 'The meeting is so already.' },
    }))
    renderVisibility(meeting)

    fireEvent.click(screen.getByRole('button', { name: 'Stop sharing with Grace Hopper' }))

    await waitFor(() => expect(routes.setMeetingShared).toHaveBeenCalledWith('review', 'grace', false))
    await waitFor(() => expect(toasts.warning.mock.calls.length + toasts.danger.mock.calls.length).toBe(1))
  })
})
