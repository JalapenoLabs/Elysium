// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'
import type { PersonPromise } from '../../api/routes/promiseRoutes'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'

// Redux
import { createAppStore } from '../../store'
import { meLoaded } from '../../store/authSlice'
import { draftUpserted } from '../../store/draftsSlice'
import { followUpPromisesLoaded } from '../../store/promisesSlice'
import { usersLoaded } from '../../store/usersSlice'

// User interface
import { ActionItemFollowUp } from './ActionItemFollowUp'

// Misc
import { makeActionItem, makeDraft, makePromise, makeUser } from '../../testFixtures'

// The draft and item requests are the network boundary; the loaders' data arrives through Redux.
const routes = vi.hoisted(() => ({
  updateDraft: vi.fn(),
  sendDraft: vi.fn(),
  transitionActionItem: vi.fn(),
}))

vi.mock('../../api/routes/draftRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/routes/draftRoutes')>(),
  updateDraft: routes.updateDraft,
  sendDraft: routes.sendDraft,
}))

vi.mock('../../api/routes/actionItemRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/routes/actionItemRoutes')>(),
  transitionActionItem: routes.transitionActionItem,
}))

const toasts = vi.hoisted(() => ({
  success: vi.fn(),
  danger: vi.fn(),
}))

vi.mock('../../toast/toast', () => ({ toast: toasts }))

vi.mock('../../hooks/useServerData', () => ({
  useFollowUpPromiseLoader: () => 'loaded',
  useDraftLoader: () => 'loaded',
  useUsersLoader: () => 'loaded',
}))

const PERSON_ID = 'person'
const ITEM = makeActionItem({ id: 'item-1', title: 'Follow up with Ada: when the login fix is live', state: 'inbox' })

function drafted(personId: string): PersonPromise {
  const promise = makePromise({
    id: 'promise-1',
    state: 'drafted',
    followUpItemId: 'item-1',
    draftId: 'draft-1',
  })
  if (!promise.madeIn) {
    throw new Error('the fixture names where it was made')
  }
  return { ...promise, madeIn: { ...promise.madeIn, personId }}
}

function renderFollowUp(item: ActionItem, promise: PersonPromise) {
  const store = createAppStore()
  store.dispatch(meLoaded({
    user: makeUser({ id: PERSON_ID }),
    hasSecondFactor: false,
    mfaEnrollmentRequired: false,
    authenticatorNickname: null,
  }))
  store.dispatch(usersLoaded([ makeUser({ id: PERSON_ID, name: 'Alex' }), makeUser({ id: 'grace', name: 'Grace' }) ]))
  store.dispatch(followUpPromisesLoaded({ itemId: item.id, promises: [ promise ]}))
  store.dispatch(draftUpserted(makeDraft({
    id: 'draft-1',
    replyToRecordId: 'record-1',
    promiseId: 'promise-1',
    body: 'The login fix is live.',
    createdBy: PERSON_ID,
  })))
  render(<Provider store={store}>
    <MemoryRouter>
      <ActionItemFollowUp item={item} />
    </MemoryRouter>
  </Provider>)
  return store
}

function button(name: string) {
  const found = screen.getByText(name).closest('button')
  if (!found) {
    throw new Error(`${name} is not a button`)
  }
  return found
}

describe('ActionItemFollowUp', () => {
  beforeEach(() => {
    for (const route of Object.values(routes)) {
      route.mockReset()
    }
    for (const shown of Object.values(toasts)) {
      shown.mockReset()
    }
    window.sessionStorage.clear()
  })

  it('shows who it is for, where it goes, and the message, and sends it as it stands', async () => {
    routes.sendDraft.mockResolvedValue({
      draft: makeDraft({ id: 'draft-1', promiseId: 'promise-1', state: 'sent', sentAt: '2026-11-04T15:00:00Z' }),
    })
    renderFollowUp(ITEM, drafted(PERSON_ID))

    expect(screen.getByText('To Ada in Reply in #launch in Mooreslab')).toBeTruthy()
    expect(screen.getByText('You promised to let them know when the login fix is live.')).toBeTruthy()
    expect(screen.getByText('The login fix is live.')).toBeTruthy()
    const conversation = screen.getByText('Open the conversation').closest('a')
    expect(conversation?.getAttribute('target')).toBe('_blank')

    fireEvent.click(button('Send'))

    await waitFor(() => expect(routes.sendDraft).toHaveBeenCalledWith('draft-1'))
    expect(routes.updateDraft).not.toHaveBeenCalled()
    await waitFor(() => expect(toasts.success).toHaveBeenCalledWith('Sent to Ada'))
  })

  it('edits the message and saves it without sending', async () => {
    routes.updateDraft.mockResolvedValue({
      draft: makeDraft({ id: 'draft-1', promiseId: 'promise-1', body: 'Fixed and live now.', createdBy: PERSON_ID }),
    })
    renderFollowUp(ITEM, drafted(PERSON_ID))

    fireEvent.click(button('Edit'))
    const editor = screen.getByRole('textbox')
    expect(editor).toHaveProperty('value', 'The login fix is live.')
    fireEvent.change(editor, { target: { value: 'Fixed and live now.' }})
    fireEvent.click(button('Save'))

    await waitFor(() => expect(routes.updateDraft).toHaveBeenCalledWith('draft-1', 'Fixed and live now.'))
    expect(routes.sendDraft).not.toHaveBeenCalled()
    expect(await screen.findByText('Fixed and live now.')).toBeTruthy()
  })

  it('dismisses it by dismissing its item', async () => {
    routes.transitionActionItem.mockResolvedValue({ item: { ...ITEM, state: 'dismissed' }})
    renderFollowUp(ITEM, drafted(PERSON_ID))

    fireEvent.click(button('Dismiss'))

    await waitFor(() => expect(routes.transitionActionItem).toHaveBeenCalledWith('item-1', 'dismiss'))
    expect(routes.sendDraft).not.toHaveBeenCalled()
  })

  it('offers nothing to do once its item is decided', () => {
    renderFollowUp({ ...ITEM, state: 'dismissed' }, drafted(PERSON_ID))
    expect(screen.getByText('The login fix is live.')).toBeTruthy()
    expect(screen.queryByText('Send')).toBeNull()
    expect(screen.queryByText('Dismiss')).toBeNull()
  })

  it('tells anyone else in the company it is its person\'s to send', () => {
    renderFollowUp(ITEM, drafted('grace'))
    expect(screen.getByText('Grace sends this follow-up.')).toBeTruthy()
    expect(screen.getByText('Grace promised to let them know when the login fix is live.')).toBeTruthy()
    expect(screen.queryByText('The login fix is live.')).toBeNull()
    expect(screen.queryByText('Send')).toBeNull()
  })
})
