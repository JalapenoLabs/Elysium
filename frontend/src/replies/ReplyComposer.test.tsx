// Copyright © 2026 Jalapeno Labs

import type { ConnectorReplies } from '../api/routes/connectorRoutes'
import type { MailAccount } from '../api/routes/mailRoutes'
import type { TimelineRecord } from '../api/routes/timelineRoutes'
import type { ToastOptions } from '../toast/toast'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'

// Redux
import { createAppStore } from '../store'
import { meLoaded } from '../store/authSlice'
import { draftUpserted } from '../store/draftsSlice'
import { mailAccountsLoaded } from '../store/mailAccountsSlice'

// User interface
import { ReplyComposer } from './ReplyComposer'

// Misc
import { REPLY_DRAFT_STORAGE_PREFIX } from '../constants'
import { makeDraft, makeMailAccount, makeTimelineRecord, makeUser } from '../testFixtures'

// The draft requests are the network boundary; the loaders' data arrives through Redux.
const routes = vi.hoisted(() => ({
  createReplyDraft: vi.fn(),
  updateDraft: vi.fn(),
  deleteDraft: vi.fn(),
  sendDraft: vi.fn(),
}))

vi.mock('../api/routes/draftRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../api/routes/draftRoutes')>(),
  ...routes,
}))

// Watched for a discard's Undo.
const toasts = vi.hoisted(() => ({
  success: vi.fn(),
  danger: vi.fn(),
}))

vi.mock('../toast/toast', () => ({ toast: toasts }))

vi.mock('../hooks/useServerData', () => ({
  useRecordDraftsLoader: () => 'loaded',
  useMailAccountsLoader: () => 'loaded',
}))

const SLACK: ConnectorReplies & { through: 'connector' } = {
  through: 'connector',
  places: [ 'thread', 'conversation' ],
}

const RECORD = makeTimelineRecord({ id: 'record-1' })

function renderComposer(
  record: TimelineRecord,
  replies: Exclude<ConnectorReplies, { through: 'never' }>,
  mailboxes: MailAccount[] = [],
) {
  const store = createAppStore()
  store.dispatch(meLoaded({
    user: makeUser({ id: 'person' }),
    hasSecondFactor: false,
    mfaEnrollmentRequired: false,
    authenticatorNickname: null,
  }))
  store.dispatch(mailAccountsLoaded(mailboxes))
  render(<Provider store={store}>
    <MemoryRouter>
      <ReplyComposer record={record} replies={replies} />
    </MemoryRouter>
  </Provider>)
  return store
}

function replyBox() {
  return screen.getByRole('textbox')
}

function sendButton() {
  const button = screen.getByText('Send').closest('button')
  if (!button) {
    throw new Error('Send is not a button')
  }
  return button
}

describe('ReplyComposer', () => {
  beforeEach(() => {
    for (const route of Object.values(routes)) {
      route.mockReset()
    }
    for (const shown of Object.values(toasts)) {
      shown.mockReset()
    }
    window.sessionStorage.clear()
  })

  it('a send that fails says why and leaves the draft in place to send again', async () => {
    routes.createReplyDraft.mockResolvedValue({
      draft: makeDraft({ id: 'draft-1', replyToRecordId: 'record-1', body: 'Friday works.' }),
    })
    routes.sendDraft.mockResolvedValue({
      draft: makeDraft({
        id: 'draft-1',
        replyToRecordId: 'record-1',
        body: 'Friday works.',
        state: 'failed',
        lastError: 'Slack did not post the message: not_in_channel',
      }),
    })
    renderComposer(RECORD, SLACK)

    fireEvent.change(replyBox(), { target: { value: 'Friday works.' }})
    fireEvent.click(sendButton())

    expect(await screen.findByText('Not sent: Slack did not post the message: not_in_channel')).toBeTruthy()
    expect(replyBox()).toHaveProperty('value', 'Friday works.')
    expect(routes.createReplyDraft).toHaveBeenCalledWith({
      recordId: 'record-1',
      place: 'thread',
      mailAccountId: undefined,
      body: 'Friday works.',
    })

    // Sending again sends the same draft rather than writing another.
    routes.sendDraft.mockResolvedValue({
      draft: makeDraft({ id: 'draft-1', replyToRecordId: 'record-1', state: 'sent', sentAt: '2026-11-03T00:00:00Z' }),
    })
    fireEvent.click(sendButton())
    await waitFor(() => expect(routes.sendDraft).toHaveBeenCalledTimes(2))
    expect(routes.createReplyDraft).toHaveBeenCalledTimes(1)
    expect(routes.sendDraft).toHaveBeenLastCalledWith('draft-1')
  })

  it('continues a failed draft, sending its edited text', async () => {
    const store = renderComposer(RECORD, SLACK)
    store.dispatch(draftUpserted(makeDraft({
      id: 'draft-1',
      replyToRecordId: 'record-1',
      body: 'Friday works.',
      state: 'failed',
      lastError: 'Slack did not post the message: not_in_channel',
    })))
    expect(await screen.findByDisplayValue('Friday works.')).toBeTruthy()

    routes.updateDraft.mockResolvedValue({
      draft: makeDraft({
        id: 'draft-1',
        replyToRecordId: 'record-1',
        body: 'Friday at noon.',
        state: 'failed',
        lastError: 'Slack did not post the message: not_in_channel',
      }),
    })
    routes.sendDraft.mockResolvedValue({
      draft: makeDraft({ id: 'draft-1', replyToRecordId: 'record-1', state: 'sent', sentAt: '2026-11-03T00:00:00Z' }),
    })
    fireEvent.change(replyBox(), { target: { value: 'Friday at noon.' }})
    fireEvent.click(sendButton())

    await waitFor(() => expect(routes.sendDraft).toHaveBeenCalledWith('draft-1'))
    expect(routes.updateDraft).toHaveBeenCalledWith('draft-1', 'Friday at noon.')
    expect(routes.createReplyDraft).not.toHaveBeenCalled()
    await waitFor(() => expect(replyBox()).toHaveProperty('value', ''))
  })

  it('discards a draft at once, and Undo puts its text back where it was going', async () => {
    routes.deleteDraft.mockResolvedValue(undefined)
    const store = renderComposer(RECORD, SLACK)
    store.dispatch(draftUpserted(makeDraft({
      id: 'draft-1',
      replyToRecordId: 'record-1',
      place: 'conversation',
      body: 'Friday works.',
      state: 'failed',
      lastError: 'Slack did not post the message: not_in_channel',
    })))
    expect(await screen.findByDisplayValue('Friday works.')).toBeTruthy()

    fireEvent.click(screen.getByText('Discard'))

    await waitFor(() => expect(replyBox()).toHaveProperty('value', ''))
    expect(routes.deleteDraft).toHaveBeenCalledWith('draft-1')
    const options: ToastOptions | undefined = toasts.success.mock.lastCall?.[1]
    expect(toasts.success).toHaveBeenLastCalledWith('Draft discarded', expect.anything())
    expect(options?.action?.label).toBe('Undo')

    act(() => options?.action?.onPress())

    expect(replyBox()).toHaveProperty('value', 'Friday works.')
    expect(window.sessionStorage.getItem(`${REPLY_DRAFT_STORAGE_PREFIX}record-1`)).toBe('Friday works.')

    // Its next send writes a new draft, in the conversation the discarded one was going to.
    routes.createReplyDraft.mockResolvedValue({
      draft: makeDraft({ id: 'draft-2', replyToRecordId: 'record-1', place: 'conversation', body: 'Friday works.' }),
    })
    routes.sendDraft.mockResolvedValue({
      draft: makeDraft({ id: 'draft-2', replyToRecordId: 'record-1', state: 'sent', sentAt: '2026-11-03T00:00:00Z' }),
    })
    fireEvent.click(sendButton())
    await waitFor(() => expect(routes.createReplyDraft).toHaveBeenCalledWith({
      recordId: 'record-1',
      place: 'conversation',
      mailAccountId: undefined,
      body: 'Friday works.',
    }))
  })

  it('keeps what was typed for the tab until it is sent', async () => {
    renderComposer(RECORD, SLACK)
    fireEvent.change(replyBox(), { target: { value: 'Half a thought' }})
    expect(window.sessionStorage.getItem(`${REPLY_DRAFT_STORAGE_PREFIX}record-1`)).toBe('Half a thought')
  })

  it('a reply by mail without a mailbox of the person\'s own says where to connect one', () => {
    const someoneElses = makeMailAccount({ id: 'mailbox-1', createdBy: 'someone-else' })
    renderComposer(makeTimelineRecord({ id: 'mail-1', sourceKind: 'gmail' }), { through: 'mailbox' }, [ someoneElses ])

    expect(screen.getByText('Replying by mail needs a mailbox of your own.')).toBeTruthy()
    expect(screen.getByText('Connect one under Email').closest('a')?.getAttribute('href')).toBe('/settings/email')
  })

  it('a reply by mail goes from the person\'s only mailbox, to the sender', async () => {
    routes.createReplyDraft.mockResolvedValue({
      draft: makeDraft({ id: 'draft-2', replyToRecordId: 'mail-1', connectorId: null, mailAccountId: 'mailbox-1' }),
    })
    routes.sendDraft.mockResolvedValue({
      draft: makeDraft({ id: 'draft-2', state: 'sent', sentAt: '2026-11-03T00:00:00Z' }),
    })
    const own = makeMailAccount({ id: 'mailbox-1', createdBy: 'person' })
    renderComposer(
      makeTimelineRecord({ id: 'mail-1', sourceKind: 'gmail', actor: 'Ada <ada@example.org>' }),
      { through: 'mailbox' },
      [ own ],
    )

    expect(screen.getByText('To Ada <ada@example.org>')).toBeTruthy()
    fireEvent.change(replyBox(), { target: { value: 'Friday works.' }})
    fireEvent.click(sendButton())

    await waitFor(() => expect(routes.createReplyDraft).toHaveBeenCalledWith({
      recordId: 'mail-1',
      place: undefined,
      mailAccountId: 'mailbox-1',
      body: 'Friday works.',
    }))
  })
})
