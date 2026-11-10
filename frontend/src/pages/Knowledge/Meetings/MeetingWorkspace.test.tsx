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
  unobserve() {}
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
    recordingContentType: null,
  }],
}

type Context = {
  applyChange: Mock<(change: MeetingTranscriptChange) => void>
}

function renderWorkspace(context: Context, shown: MeetingWords = transcript) {
  const store = createAppStore()
  const router = createMemoryRouter([{
    path: '*',
    element: <Provider store={store}>
      <SWRConfig value={{ provider: () => new Map() }}>
        <TestAriaRouter>
          <MeetingWorkspace
            meeting={makeMeeting({ id: 'meeting' })}
            transcript={shown}
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

// The same meeting with an audio and a video recording, the audio's clock five seconds along the
// meeting's.
const recorded: MeetingWords = {
  ...transcript,
  captures: [
    {
      id: 'audio',
      connectorId: null,
      sourceKind: 'fireflies',
      url: null,
      offsetMs: 5000,
      hasRecording: true,
      recordingContentType: 'audio/mpeg',
    },
    {
      id: 'video',
      connectorId: null,
      sourceKind: 'fireflies',
      url: null,
      offsetMs: 0,
      hasRecording: true,
      recordingContentType: 'video/mp4',
    },
  ],
}

function playerMedia() {
  const media = document.querySelector('video')
  if (!media) {
    throw new Error('no player')
  }
  return media
}

describe('MeetingWorkspace player', () => {
  beforeEach<Context>((context) => {
    vi.stubGlobal('ResizeObserver', InertObserver)
    vi.stubGlobal('IntersectionObserver', InertObserver)
    // jsdom loads no media, so playing is what the page asks for.
    vi.spyOn(HTMLMediaElement.prototype, 'play').mockResolvedValue()
    vi.spyOn(HTMLMediaElement.prototype, 'pause').mockImplementation(() => {})
    context.applyChange = vi.fn()
    vi.mocked(getMeetingHistory).mockResolvedValue({ entries: []})
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.mocked(HTMLMediaElement.prototype.play).mockRestore()
    vi.mocked(HTMLMediaElement.prototype.pause).mockRestore()
  })

  it<Context>('starts on the video, streamed from its capture\'s media route', (context) => {
    renderWorkspace(context, recorded)

    expect(playerMedia().getAttribute('src')).toBe('/api/v1/meetings/meeting/captures/video/media')
  })

  it<Context>('plays nothing for a meeting with no recording, leaving Space to the page', (context) => {
    renderWorkspace(context)

    expect(document.querySelector('video')).toBeNull()
    // Not prevented: the page scrolls as Space always does.
    expect(fireEvent.keyDown(document.body, { key: ' ' })).toBe(true)
  })

  it<Context>('seeks to a word clicked, on the meeting\'s clock', (context) => {
    renderWorkspace(context, recorded)

    fireEvent.click(wordPiece('Fryday.'))

    expect(playerMedia().currentTime).toBe(2)
  })

  it<Context>('plays and pauses on Space, and jumps ten seconds on the arrows', (context) => {
    renderWorkspace(context, recorded)
    const media = playerMedia()

    press(' ')
    expect(HTMLMediaElement.prototype.play).toHaveBeenCalledTimes(1)
    Object.defineProperty(media, 'paused', { configurable: true, value: false })
    fireEvent.play(media)
    press(' ')
    expect(HTMLMediaElement.prototype.pause).toHaveBeenCalledTimes(1)

    media.currentTime = 30
    press('ArrowRight')
    expect(media.currentTime).toBe(40)
    press('ArrowLeft')
    press('ArrowLeft')
    expect(media.currentTime).toBe(20)
  })

  it<Context>('leaves Space to a button that has focus, and the arrows to the pills', (context) => {
    renderWorkspace(context, recorded)
    fireEvent.click(wordPiece('Fryday.'))
    const media = playerMedia()
    media.currentTime = 30

    fireEvent.keyDown(screen.getByRole('button', { name: /Play from here/ }), { key: ' ' })
    fireEvent.keyDown(screen.getByRole('radio', { name: 'Confidence' }), { key: 'ArrowRight' })

    expect(HTMLMediaElement.prototype.play).not.toHaveBeenCalled()
    expect(media.currentTime).toBe(30)
  })

  it<Context>('plays from the words selected, from Space or Play from here', (context) => {
    renderWorkspace(context, recorded)
    const media = playerMedia()
    fireEvent.click(wordPiece('is'))
    fireEvent.click(wordPiece('Agreed.'), { shiftKey: true })
    media.currentTime = 30

    press(' ')
    expect(media.currentTime).toBe(1)
    expect(HTMLMediaElement.prototype.play).toHaveBeenCalledTimes(1)

    media.currentTime = 30
    fireEvent.click(screen.getByRole('button', { name: /Play from here/ }))
    expect(media.currentTime).toBe(1)
    expect(HTMLMediaElement.prototype.play).toHaveBeenCalledTimes(2)
  })

  it<Context>('underlines the word being spoken once the player has moved', (context) => {
    renderWorkspace(context, recorded)
    const media = playerMedia()
    expect(document.querySelector('[data-spoken]')).toBeNull()

    media.currentTime = 1.5
    fireEvent.seeked(media)

    expect(wordPiece('is').hasAttribute('data-spoken')).toBe(true)
  })

  it<Context>('switches capture at the same moment on the meeting\'s clock', async (context) => {
    renderWorkspace(context, recorded)
    const media = playerMedia()
    media.currentTime = 12

    fireEvent.click(screen.getByRole('button', { name: /Recording/ }))
    fireEvent.click(await screen.findByRole('option', { name: 'Fireflies 1' }))
    expect(media.getAttribute('src')).toBe('/api/v1/meetings/meeting/captures/audio/media')
    fireEvent.loadedMetadata(media)

    // Twelve seconds into the meeting is seven into a recording that began five seconds in.
    expect(media.currentTime).toBe(7)
  })
})
