// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { PageSaveState } from '../../../layout/page/pageSave'

// Core
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { HTTPError } from 'ky'
import { Provider } from 'react-redux'
import { SWRConfig } from 'swr'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'
import { selectOpenedPage } from '../../../store/pagesSlice'

// User interface
import { RestoreVersionButton } from './RestoreVersionButton'

// Misc
import { PageSaveStateContext } from '../../../layout/page/pageSave'
import { makePage } from '../../../testFixtures'

const routes = vi.hoisted(() => ({ restorePageVersion: vi.fn() }))

vi.mock('../../../api/routes/pageRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/pageRoutes')>(),
  restorePageVersion: routes.restorePageVersion,
}))

const toasts = vi.hoisted(() => Object.assign(vi.fn(), {
  success: vi.fn(),
  warning: vi.fn(),
  danger: vi.fn(),
}))

vi.mock('../../../toast/toast', () => ({ toast: toasts }))

const PAGE_ID = 'page-1'

function refusal(code: string, message: string) {
  const response = new Response(null, { status: 409 })
  const error = new HTTPError(response, new Request('http://localhost/'), {} as never)
  error.data = { code, message }
  return error
}

function renderButton(wrap: (button: ReactNode) => ReactNode = (button) => button) {
  const store = createAppStore()
  render(<SWRConfig value={{ provider: () => new Map() }}>
    <Provider store={store}>{
      wrap(<RestoreVersionButton pageId={PAGE_ID} version={2} />)
    }</Provider>
  </SWRConfig>)
  return store
}

describe('RestoreVersionButton', () => {
  beforeEach(() => {
    routes.restorePageVersion.mockReset()
    toasts.mockClear()
    toasts.success.mockClear()
    toasts.warning.mockClear()
    toasts.danger.mockClear()
  })

  it('restores at once, without asking, and holds the page it answers', async () => {
    routes.restorePageVersion.mockResolvedValue({
      page: makePage({ id: PAGE_ID, currentVersion: 6, body: 'Restored' }),
      outcome: 'published',
    })

    const store = renderButton()
    fireEvent.click(screen.getByRole('button', { name: 'Restore version 2' }))

    await waitFor(() => expect(toasts.success).toHaveBeenCalled())
    expect(routes.restorePageVersion).toHaveBeenCalledWith(PAGE_ID, 2)
    expect(selectOpenedPage(store.getState(), PAGE_ID)?.body).toBe('Restored')
    expect(toasts.success.mock.calls[0][0]).toBe('Version 2 restored as version 6')
  })

  it('undoes by restoring the version it replaced, which loses nothing', async () => {
    routes.restorePageVersion.mockResolvedValue({
      page: makePage({ id: PAGE_ID, currentVersion: 6 }),
      outcome: 'published',
    })

    renderButton()
    fireEvent.click(screen.getByRole('button', { name: 'Restore version 2' }))
    await waitFor(() => expect(toasts.success).toHaveBeenCalled())

    routes.restorePageVersion.mockResolvedValue({
      page: makePage({ id: PAGE_ID, currentVersion: 7 }),
      outcome: 'published',
    })
    const { action } = toasts.success.mock.calls[0][1]
    expect(action.label).toBe('Undo')
    action.onPress()

    await waitFor(() => expect(routes.restorePageVersion).toHaveBeenLastCalledWith(PAGE_ID, 5))
  })

  it('says so when the page already holds that version, with nothing to undo', async () => {
    routes.restorePageVersion.mockResolvedValue({
      page: makePage({ id: PAGE_ID, currentVersion: 5 }),
      outcome: 'unchanged',
    })

    renderButton()
    fireEvent.click(screen.getByRole('button', { name: 'Restore version 2' }))

    await waitFor(() => expect(toasts).toHaveBeenCalledWith('The page already says what version 2 says'))
    expect(toasts.success).not.toHaveBeenCalled()
  })

  it('explains a learning\'s title taken by another learning', async () => {
    vi.spyOn(console, 'debug').mockImplementationOnce(() => {})
    routes.restorePageVersion.mockRejectedValue(refusal('page_title_taken', 'in the API\'s words'))

    renderButton()
    fireEvent.click(screen.getByRole('button', { name: 'Restore version 2' }))

    await waitFor(() => expect(toasts.warning).toHaveBeenCalledWith('Another learning has this version\'s title now'))
  })

  it('shows any other refusal in the API\'s words', async () => {
    vi.spyOn(console, 'debug').mockImplementationOnce(() => {})
    routes.restorePageVersion.mockRejectedValue(refusal('page_section_too_long', 'That is too long'))

    renderButton()
    fireEvent.click(screen.getByRole('button', { name: 'Restore version 2' }))

    await waitFor(() => expect(toasts.danger).toHaveBeenCalledWith(
      'Something went wrong. Please try again.',
      { description: 'That is too long' },
    ))
  })

  it('waits for unsaved edits, since it would change the page under them', () => {
    const editing = {
      save: { save: async () => true, isDirty: true },
    } as PageSaveState

    renderButton((button) => <PageSaveStateContext.Provider value={editing}>{button}</PageSaveStateContext.Provider>)

    expect(screen.getByRole<HTMLButtonElement>('button', { name: 'Restore version 2' }).disabled).toBe(true)
  })
})
