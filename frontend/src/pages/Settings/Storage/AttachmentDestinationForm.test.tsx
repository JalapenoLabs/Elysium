// Copyright © 2026 Jalapeno Labs

// Core
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { SWRConfig } from 'swr'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'
import { meLoaded } from '../../../store/authSlice'

// User interface
import { SettingsPage } from '../SettingsPage'
import { AttachmentDestinationForm } from './AttachmentDestinationForm'

// Misc
import { setMeetingAttachmentLimit } from '../../../api/routes/meetingAttachmentRoutes'
import { choosePageAttachmentDestination } from '../../../api/routes/pageRoutes'
import { makeMeetingAttachmentPolicy, makeUser } from '../../../testFixtures'
import { TestAriaRouter } from '../../../testRouter'

vi.mock('../../../api/routes/meetingAttachmentRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/meetingAttachmentRoutes')>(),
  setMeetingAttachmentLimit: vi.fn(),
}))

vi.mock('../../../api/routes/pageRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/pageRoutes')>(),
  choosePageAttachmentDestination: vi.fn(),
}))

vi.mock('../../../toast/toast', () => ({
  toast: Object.assign(vi.fn(), { success: vi.fn(), warning: vi.fn(), danger: vi.fn() }),
}))

// jsdom has neither observer; the page uses them only to place the floating Save card and to lay out
// HeroUI's fields.
class InertObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

function renderForm(onSaved = vi.fn()) {
  const router = createMemoryRouter([{
    path: '*',
    element: <TestAriaRouter>
      <SettingsPage title='Attachments'>
        <AttachmentDestinationForm
          destinationId={null}
          meetingMaxBytes={1_000_000_000}
          meetingMaxBytesCeiling={5_368_709_120}
          locations={[]}
          onSaved={onSaved}
        />
      </SettingsPage>
    </TestAriaRouter>,
  }], { initialEntries: [ '/settings/storage/attachments' ]})
  const store = createAppStore()
  store.dispatch(meLoaded({
    user: makeUser(),
    hasSecondFactor: false,
    mfaEnrollmentRequired: false,
    authenticatorNickname: null,
  }))
  render(<SWRConfig value={{ provider: () => new Map() }}>
    <Provider store={store}>
      <RouterProvider router={router} />
    </Provider>
  </SWRConfig>)
  return onSaved
}

describe('AttachmentDestinationForm', () => {
  beforeEach(() => {
    vi.stubGlobal('IntersectionObserver', InertObserver)
    vi.stubGlobal('ResizeObserver', InertObserver)
    vi.mocked(setMeetingAttachmentLimit).mockReset()
    vi.mocked(choosePageAttachmentDestination).mockReset()
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('saves the largest file a meeting takes in bytes, leaving the location as it was', async () => {
    vi.mocked(setMeetingAttachmentLimit).mockResolvedValue({
      policy: makeMeetingAttachmentPolicy({ maxBytes: 2_500_000_000 }),
    })
    const onSaved = renderForm()

    const field = screen.getByRole('textbox', { name: 'Largest file a meeting takes' })
    act(() => field.focus())
    fireEvent.change(field, { target: { value: '2.5' } })
    act(() => field.blur())
    fireEvent.click(screen.getAllByRole('button', { name: 'Save' })[0])

    await waitFor(() => expect(setMeetingAttachmentLimit).toHaveBeenCalledWith(2_500_000_000))
    expect(choosePageAttachmentDestination).not.toHaveBeenCalled()
    await waitFor(() => expect(onSaved).toHaveBeenCalled())
  })
})
