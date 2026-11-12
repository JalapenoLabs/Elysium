// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../../api/routes/meetingRoutes'
import type { ToastOptions } from '../../../toast/toast'

// Core
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { SWRConfig } from 'swr'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'
import { useAppSelector } from '../../../store/hooks'
import { meetingUpserted, selectMeetingById } from '../../../store/meetingsSlice'

// User interface
import { DeletedMeetingNotice } from './DeletedMeetingNotice'
import { MeetingActions } from './MeetingActions'

// Misc
import { makeMeeting } from '../../../testFixtures'
import { getMeetingUrl, UrlTree } from '../../../urls'

// Deleting and restoring are the network boundary; toasts are watched for their Undo.
const routes = vi.hoisted(() => ({
  changeMeetings: vi.fn(),
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

const DELETED_AT = '2026-11-11T09:00:00Z'
const MEETING = makeMeeting({ id: 'meeting', title: 'Duplicate sync' })

// The meeting's page, holding its actions and, while it is deleted, its notice, beside the list it
// leaves for.
function renderPage(meeting: Meeting) {
  const store = createAppStore()
  store.dispatch(meetingUpserted(meeting))
  function OnPage() {
    const shown = useAppSelector((state) => selectMeetingById(state, meeting.id)) ?? meeting
    return <>
      {shown.deletedAt && <DeletedMeetingNotice meeting={shown} />}
      <MeetingActions meeting={shown} />
    </>
  }
  const router = createMemoryRouter([
    { path: UrlTree.knowledgeMeetings, element: <p>The Meetings page</p> },
    { path: '*', element: <OnPage /> },
  ], { initialEntries: [ getMeetingUrl(meeting.id) ]})
  render(<Provider store={store}>
    <SWRConfig value={{ provider: () => new Map() }}>
      <RouterProvider router={router} />
    </SWRConfig>
  </Provider>)
  return { store, router }
}

function answerWith(meeting: Meeting, deletedAt: string | null) {
  return { meetings: [{ ...meeting, deletedAt }], refused: []}
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('MeetingActions', () => {
  it('deletes the meeting from its page at once, leaving for the list, with Undo coming back', async () => {
    routes.changeMeetings.mockResolvedValueOnce(answerWith(MEETING, DELETED_AT))
    const { store, router } = renderPage(MEETING)

    // Destructive, so last in the actions menu, below everything else.
    fireEvent.click(screen.getByRole('button', { name: 'Actions' }))
    const items = await screen.findAllByRole('menuitem')
    expect(items.at(-1)?.textContent).toBe('Delete')
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete' }))

    await waitFor(() => expect(router.state.location.pathname).toBe(UrlTree.knowledgeMeetings))
    expect(routes.changeMeetings).toHaveBeenCalledWith([ 'meeting' ], 'delete')
    expect(store.getState().meetings.entities.meeting?.deletedAt).toBe(DELETED_AT)
    const options: ToastOptions | undefined = toasts.success.mock.lastCall?.[1]
    expect(toasts.success.mock.lastCall?.[0]).toBe('Deleted “Duplicate sync”.')

    routes.changeMeetings.mockResolvedValueOnce(answerWith(MEETING, null))
    await act(async () => options?.action?.onPress())
    await waitFor(() => expect(router.state.location.pathname).toBe(getMeetingUrl('meeting')))
    expect(routes.changeMeetings).toHaveBeenLastCalledWith([ 'meeting' ], 'restore')
    expect(store.getState().meetings.entities.meeting?.deletedAt).toBeNull()
  })

  it('offers a deleted meeting nothing to change, and its notice restores it', async () => {
    const deleted = { ...MEETING, deletedAt: DELETED_AT }
    routes.changeMeetings.mockResolvedValueOnce(answerWith(MEETING, null))
    const { store } = renderPage(deleted)

    expect(screen.getByText('This meeting is deleted')).toBeTruthy()
    expect(screen.getByText('Restore it to change it.')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Actions' })).toBeNull()
    expect(screen.queryByText('Add a recording or transcript')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Restore' }))

    await waitFor(() => expect(store.getState().meetings.entities.meeting?.deletedAt).toBeNull())
    expect(routes.changeMeetings).toHaveBeenCalledWith([ 'meeting' ], 'restore')
    expect(toasts.success).toHaveBeenCalledWith('Restored “Duplicate sync”.')
  })
})
