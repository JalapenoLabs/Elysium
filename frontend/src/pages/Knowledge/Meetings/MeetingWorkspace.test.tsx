// Copyright © 2026 Jalapeno Labs

import type { Mock } from 'vitest'
import type {
  MeetingMark,
  MeetingTranscriptChange,
  MeetingWord,
  MeetingWords,
} from '../../../api/routes/meetingRoutes'

// Core
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { SWRConfig } from 'swr'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'

// User interface
import { MeetingWorkspace } from './MeetingWorkspace'

// Misc
import {
  editMeetingWords,
  getCaptureTranscript,
  getMeetingHistory,
  undoMeetingChange,
} from '../../../api/routes/meetingRoutes'
import { makeMeeting } from '../../../testFixtures'
import { TestAriaRouter } from '../../../testRouter'
import { toast } from '../../../toast/toast'

vi.mock('../../../api/routes/meetingRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/meetingRoutes')>(),
  editMeetingWords: vi.fn(),
  undoMeetingChange: vi.fn(),
  getMeetingHistory: vi.fn(),
  getCaptureTranscript: vi.fn(),
}))

vi.mock('../../../hooks/useServerData', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../hooks/useServerData')>(),
  useUsersLoader: () => 'loaded',
  useProjectsLoader: () => 'loaded',
}))

vi.mock('../../../toast/toast', () => ({
  toast: Object.assign(vi.fn(), { success: vi.fn(), warning: vi.fn(), danger: vi.fn() }),
}))

class InertObserver {
  observe() {}
  disconnect() {}
}

function word(id: string, speaker: string, text: string, index: number): MeetingWord {
  return {
    id,
    position: (index + 1) * 1024,
    text,
    speaker,
    startMs: index * 1000,
    endMs: index * 1000 + 1000,
    origin: 'source',
    captureId: 'capture',
  }
}

function mark(id: string, kind: string, startWordId: string, endWordId: string, data = {}): MeetingMark {
  return {
    id,
    kind,
    startWordId,
    endWordId,
    data,
    createdBy: 'person',
    agent: null,
    createdAt: '2026-11-10T12:00:00Z',
  }
}

const transcript: MeetingWords = {
  words: [
    word('w0', 'Ada', 'Launch', 0),
    word('w1', 'Ada', 'is', 1),
    word('w2', 'Ada', 'Fryday.', 2),
    word('w3', 'Grace', 'Agreed.', 3),
  ],
  marks: [
    mark('unsure', 'confidence', 'w2', 'w2', { confidence: 20, alternatives: [ 'Friday.' ]}),
    mark('note', 'comment', 'w1', 'w2'),
  ],
  captures: [{
    id: 'capture',
    connectorId: null,
    sourceKind: 'fireflies',
    url: null,
    offsetMs: 0,
    hasRecording: false,
  }],
}

type Context = {
  applyChange: Mock<(change: MeetingTranscriptChange) => void>
}

function renderWorkspace(context: Context) {
  const store = createAppStore()
  const router = createMemoryRouter([{
    path: '*',
    element: <Provider store={store}>
      <SWRConfig value={{ provider: () => new Map() }}>
        <TestAriaRouter>
          <MeetingWorkspace
            meeting={makeMeeting({ id: 'meeting' })}
            transcript={transcript}
            applyChange={context.applyChange}
            reload={() => {}}
          />
        </TestAriaRouter>
      </SWRConfig>
    </Provider>,
  }])
  render(<RouterProvider router={router} />)
}

// The word as the transcript draws it, by what it says.
function wordPiece(text: string) {
  const piece = screen.getAllByText(text).find((element) => element.hasAttribute('data-word-index'))
  if (!piece) {
    throw new Error(`no word ${text}`)
  }
  return piece
}

function press(key: string, options: KeyboardEventInit = {}) {
  fireEvent.keyDown(document.body, { key, ...options })
}

describe('MeetingWorkspace', () => {
  beforeEach<Context>((context) => {
    vi.stubGlobal('ResizeObserver', InertObserver)
    context.applyChange = vi.fn()
    vi.mocked(editMeetingWords).mockReset()
    vi.mocked(undoMeetingChange).mockReset()
    vi.mocked(getMeetingHistory).mockResolvedValue({ entries: []})
    vi.mocked(getCaptureTranscript).mockReset()
    vi.mocked(toast.success).mockReset()
    vi.mocked(toast).mockReset()
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it<Context>('picks a moment where a click lands on no mark, and selects nothing', (context) => {
    renderWorkspace(context)

    fireEvent.click(wordPiece('Launch'))

    expect(wordPiece('Launch').hasAttribute('data-moment')).toBe(true)
    expect(screen.getByText('At 0:00')).toBeTruthy()
    expect(screen.queryByRole('button', { name: /Edit/ })).toBeNull()
  })

  it<Context>('selects the shortest mark under a click, shows every mark there, and Tab moves on', (context) => {
    renderWorkspace(context)

    fireEvent.click(wordPiece('Fryday.'))

    const pills = within(screen.getByRole('radiogroup', { name: 'Marks here' }))
    expect(pills.getByRole('radio', { name: 'Confidence' }).getAttribute('aria-checked')).toBe('true')
    expect(screen.getByText('20% sure')).toBeTruthy()

    press('Tab')
    expect(pills.getByRole('radio', { name: 'Comment' }).getAttribute('aria-checked')).toBe('true')
    // The comment covers two words, which are what is selected now.
    expect(wordPiece('is').hasAttribute('data-selected')).toBe(true)
  })

  it<Context>('leaves Tab and E to the rest of the page, where focus moves as everywhere', (context) => {
    renderWorkspace(context)
    fireEvent.click(wordPiece('Fryday.'))

    // A key pressed on a switch in the lane, outside the transcript.
    const elsewhere = screen.getByRole('switch', { name: 'Edit' })
    const tab = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })
    act(() => {
      elsewhere.dispatchEvent(tab)
    })
    fireEvent.keyDown(elsewhere, { key: 'e' })

    expect(tab.defaultPrevented).toBe(false)
    const pills = within(screen.getByRole('radiogroup', { name: 'Marks here' }))
    expect(pills.getByRole('radio', { name: 'Confidence' }).getAttribute('aria-checked')).toBe('true')
    expect(screen.queryByRole('textbox', { name: 'What these words should say' })).toBeNull()
  })

  it<Context>('edits words selected by Shift+click, applies with Ctrl+Enter, and Ctrl+Z undoes it', async (context) => {
    vi.mocked(editMeetingWords).mockResolvedValue({
      entry: {
        id: 'entry',
        kind: 'edited',
        createdBy: 'person',
        agent: null,
        data: {},
        undid: null,
        createdAt: '2026-11-10T12:00:00Z',
      },
      change: { removedWordIds: [ 'w0', 'w1' ], words: [], marks: []},
    })
    vi.mocked(undoMeetingChange).mockResolvedValue({
      entry: {
        id: 'undo',
        kind: 'edited',
        createdBy: 'person',
        agent: null,
        data: {},
        undid: 'entry',
        createdAt: '2026-11-10T12:00:01Z',
      },
      change: { removedWordIds: [], words: [], marks: []},
    })
    renderWorkspace(context)

    fireEvent.click(wordPiece('Launch'))
    fireEvent.click(wordPiece('is'), { shiftKey: true })
    expect(screen.getByText('0:00 to 0:02, 2s, 2 words')).toBeTruthy()

    press('e')
    const field = await screen.findByRole('textbox', { name: 'What these words should say' })
    expect((field as HTMLTextAreaElement).value).toBe('Launch is')
    fireEvent.change(field, { target: { value: 'The launch is' }})
    fireEvent.keyDown(field, { key: 'Enter', ctrlKey: true })

    await waitFor(() => expect(editMeetingWords).toHaveBeenCalledWith('meeting', {
      startWordId: 'w0',
      endWordId: 'w1',
      text: 'The launch is',
    }))
    expect(context.applyChange).toHaveBeenCalledWith({ removedWordIds: [ 'w0', 'w1' ], words: [], marks: []})
    expect(vi.mocked(toast.success).mock.calls[0][1]?.action?.label).toBe('Undo')

    await act(async () => press('z', { ctrlKey: true }))
    await waitFor(() => expect(undoMeetingChange).toHaveBeenCalledWith('meeting', 'entry'))

    // What was undone is not the page's to undo again; Ctrl+Z goes no further.
    await act(async () => press('z', { ctrlKey: true }))
    expect(undoMeetingChange).toHaveBeenCalledTimes(1)
    expect(toast).toHaveBeenCalledWith('Nothing to undo yet')
  })

  it<Context>('offers the agent\'s alternatives as one-click choices', async (context) => {
    vi.mocked(editMeetingWords).mockRejectedValue(new Error('refused'))
    renderWorkspace(context)

    fireEvent.click(wordPiece('Fryday.'))
    press('e')
    fireEvent.click(await screen.findByRole('button', { name: 'Friday.' }))

    await waitFor(() => expect(editMeetingWords).toHaveBeenCalledWith('meeting', {
      startWordId: 'w2',
      endWordId: 'w2',
      text: 'Friday.',
    }))
    // Refused, the editor stays open with the words to try again.
    expect(screen.getByRole('textbox', { name: 'What these words should say' })).toBeTruthy()
  })

  it<Context>('clears what is picked on Esc, before the page could go up a level', (context) => {
    renderWorkspace(context)
    fireEvent.click(wordPiece('Fryday.'))

    const event = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    act(() => {
      document.body.dispatchEvent(event)
    })

    expect(event.defaultPrevented).toBe(true)
    expect(wordPiece('Fryday.').hasAttribute('data-selected')).toBe(false)
    expect(screen.queryByRole('radiogroup', { name: 'Marks here' })).toBeNull()
  })

  it<Context>('draws each kind of mark over its words until its switch is off', (context) => {
    renderWorkspace(context)

    expect(wordPiece('Fryday.').getAttribute('data-confidence')).toBe('20')
    expect(wordPiece('is').hasAttribute('data-comment-start')).toBe(true)

    fireEvent.click(screen.getByRole('switch', { name: 'Comment' }))
    expect(document.querySelector('[data-comment]')).toBeNull()
    expect(wordPiece('Fryday.').getAttribute('data-confidence')).toBe('20')
  })

  it<Context>('reads a capture exactly as it came from the options', async (context) => {
    vi.mocked(getCaptureTranscript).mockResolvedValue({
      transcript: {
        basedOn: 'source',
        lines: [{ startMs: 0, endMs: 3000, speaker: 'Ada', text: 'Launch is Fryday.' }],
      },
    })
    renderWorkspace(context)

    fireEvent.click(screen.getByRole('radio', { name: 'Fireflies' }))

    expect(await screen.findByText('Launch is Fryday.')).toBeTruthy()
    expect(getCaptureTranscript).toHaveBeenCalledWith('meeting', 'capture')
    expect(document.querySelector('[data-word-index]')).toBeNull()
  })
})
