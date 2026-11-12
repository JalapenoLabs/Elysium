// Copyright © 2026 Jalapeno Labs

import type { Mock } from 'vitest'
import type {
  Meeting,
  MeetingComment,
  MeetingHistoryEntry,
  MeetingMark,
  MeetingTranscriptChange,
  MeetingWord,
  MeetingWords,
} from '../../../api/routes/meetingRoutes'
import type { Project } from '../../../api/routes/projectRoutes'
import type { TimelineRecord } from '../../../api/routes/timelineRoutes'
import type { User } from '../../../api/routes/userRoutes'

// Core
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { useState } from 'react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { SWRConfig } from 'swr'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'
import { meLoaded } from '../../../store/authSlice'
import { projectsLoaded } from '../../../store/projectsSlice'
import { usersLoaded } from '../../../store/usersSlice'

// User interface
import { MeetingWorkspace } from './MeetingWorkspace'

// Misc
import {
  assignMeetingExcerpt,
  blackOutMeetingWords,
  editMeetingWords,
  getCaptureTranscript,
  getMeetingHistory,
  getMeetingThreads,
  removeMeetingMark,
  setMeetingThreadResolved,
  startMeetingThread,
  undoMeetingChange,
} from '../../../api/routes/meetingRoutes'
import { getTimelineRecord } from '../../../api/routes/timelineRoutes'
import { ELYSIA_USER_ID } from '../../../constants'
import { makeMeeting, makeProject, makeUser } from '../../../testFixtures'
import { TestAriaRouter } from '../../../testRouter'
import { toast } from '../../../toast/toast'

vi.mock('../../../api/routes/meetingRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/meetingRoutes')>(),
  editMeetingWords: vi.fn(),
  assignMeetingExcerpt: vi.fn(),
  blackOutMeetingWords: vi.fn(),
  removeMeetingMark: vi.fn(),
  undoMeetingChange: vi.fn(),
  getMeetingHistory: vi.fn(),
  getCaptureTranscript: vi.fn(),
  getMeetingThreads: vi.fn(),
  startMeetingThread: vi.fn(),
  setMeetingThreadResolved: vi.fn(),
}))

vi.mock('../../../api/routes/timelineRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/timelineRoutes')>(),
  getTimelineRecord: vi.fn(),
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
    excerpt: null,
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
    offsetSource: 'listed',
    hasRecording: false,
    recordingContentType: null,
    mergedFrom: null,
  }],
}

type Context = {
  applyChange: Mock<(change: MeetingTranscriptChange) => void>
}

const people: User[] = [
  {
    id: 'person',
    kind: 'person',
    name: 'Ada Lovelace',
    email: 'ada@example.com',
    role: 'admin',
    status: 'active',
    approvedAt: null,
    lastSeenAt: null,
    createdAt: '2026-11-10T12:00:00Z',
    updatedAt: '2026-11-10T12:00:00Z',
  },
  {
    id: ELYSIA_USER_ID,
    kind: 'machine',
    name: 'Elysia',
    email: null,
    role: 'agent',
    status: null,
    approvedAt: null,
    lastSeenAt: null,
    createdAt: '2026-11-10T12:00:00Z',
    updatedAt: '2026-11-10T12:00:00Z',
  },
]

function entry(id: string, kind: string): MeetingHistoryEntry {
  return {
    id,
    kind,
    createdBy: 'person',
    agent: null,
    data: {},
    undid: null,
    createdAt: '2026-11-10T12:00:00Z',
  }
}

function comment(id: string, threadId: string, body: string): MeetingComment {
  return {
    id,
    threadId,
    body,
    mentions: [],
    createdBy: 'person',
    agent: null,
    createdAt: '2026-11-10T12:00:00Z',
  }
}

type HarnessProps = {
  context: Context
  meeting: Meeting
  initial: MeetingWords
  // Hands the test a way to read the transcript again, as `meeting.changed` does.
  onReady: (show: (next: MeetingWords) => void) => void
}

function Harness(props: HarnessProps) {
  const [ shown, setShown ] = useState(props.initial)
  props.onReady(setShown)
  return <MeetingWorkspace
    meeting={props.meeting}
    transcript={shown}
    applyChange={props.context.applyChange}
    reload={() => {}}
  />
}

// Renders `meeting`'s workspace on `shown` at `address`, with `projects` loaded, and answers a way
// to show it another transcript in the same session.
function renderWorkspace(
  context: Context,
  shown: MeetingWords = transcript,
  address = '/',
  {
    projects = [],
    meeting = makeMeeting({ id: 'meeting' }),
    readerId = meeting.ownerId,
  }: { projects?: Project[], meeting?: Meeting, readerId?: string } = {},
) {
  const store = createAppStore()
  // The reader is the meeting's owner unless a test names another.
  store.dispatch(meLoaded({
    user: makeUser({ id: readerId }),
    hasSecondFactor: false,
    mfaEnrollmentRequired: false,
    authenticatorNickname: null,
  }))
  store.dispatch(usersLoaded(people))
  store.dispatch(projectsLoaded(projects))
  let show: (next: MeetingWords) => void = () => {}
  const router = createMemoryRouter([{
    path: '*',
    element: <Provider store={store}>
      <SWRConfig value={{ provider: () => new Map() }}>
        <TestAriaRouter>
          <Harness
            context={context}
            meeting={meeting}
            initial={shown}
            onReady={(setShown) => {
              show = setShown
            }}
          />
        </TestAriaRouter>
      </SWRConfig>
    </Provider>,
  }], { initialEntries: [ address ]})
  render(<RouterProvider router={router} />)
  return (next: MeetingWords) => act(() => show(next))
}

// Excerpt `launch`'s record, on `projectIds`, as its lane reads it.
function excerptRecord(projectIds: string[]): TimelineRecord {
  return {
    id: 'record',
    sourceKind: 'meeting',
    connectorId: null,
    externalId: 'excerpt:launch',
    occurredAt: '2026-11-10T12:00:00Z',
    actor: 'Ada',
    title: 'The launch moves',
    body: '[0:00] Ada: Launch is\n',
    url: null,
    media: null,
    metadata: {},
    actionItemId: null,
    projects: projectIds.map((projectId) => ({
      projectId,
      confidence: 1,
      attributedBy: 'user:person',
      createdAt: '2026-11-10T12:00:00Z',
    })),
    createdBy: 'person',
    createdAt: '2026-11-10T12:00:00Z',
    excerptMarkId: 'launch',
  }
}

// The word as the transcript draws it, by what it says.
function wordPiece(text: string) {
  const piece = screen.getAllByText(text).find((element) => element.hasAttribute('data-word-index'))
  if (!piece) {
    throw new Error(`no word ${text}`)
  }
  return piece
}

// A key pressed where focus is, as a browser sends it.
function press(key: string, options: KeyboardEventInit = {}) {
  fireEvent.keyDown(document.activeElement ?? document.body, { key, ...options })
}

describe('MeetingWorkspace', () => {
  beforeEach<Context>((context) => {
    vi.stubGlobal('ResizeObserver', InertObserver)
    context.applyChange = vi.fn()
    vi.mocked(editMeetingWords).mockReset()
    vi.mocked(assignMeetingExcerpt).mockReset()
    vi.mocked(blackOutMeetingWords).mockReset()
    vi.mocked(removeMeetingMark).mockReset()
    vi.mocked(getTimelineRecord).mockReset()
    vi.mocked(undoMeetingChange).mockReset()
    vi.mocked(getMeetingHistory).mockResolvedValue({ entries: []})
    vi.mocked(getCaptureTranscript).mockReset()
    vi.mocked(getMeetingThreads).mockResolvedValue({ comments: []})
    vi.mocked(startMeetingThread).mockReset()
    vi.mocked(setMeetingThreadResolved).mockReset()
    // Comment drafts are kept for the tab, which every test here shares.
    window.sessionStorage.clear()
    vi.mocked(toast.success).mockReset()
    vi.mocked(toast).mockReset()
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.restoreAllMocks()
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
    // A mark that is no excerpt has no record, so nothing in its detail waits for one.
    expect(screen.queryByLabelText('Loading its projects')).toBeNull()
    expect(getTimelineRecord).not.toHaveBeenCalled()

    press('Tab')
    expect(pills.getByRole('radio', { name: 'Comment' }).getAttribute('aria-checked')).toBe('true')
    // The comment covers two words, which are what is selected now.
    expect(wordPiece('is').hasAttribute('data-selected')).toBe(true)
  })

  it<Context>('leaves Tab, E, and C to the rest of the page, where focus moves as everywhere', (context) => {
    renderWorkspace(context)
    fireEvent.click(wordPiece('Fryday.'))

    // A key pressed on a switch in the lane, outside the transcript.
    const elsewhere = screen.getByRole('switch', { name: 'Edit' })
    const tab = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })
    act(() => {
      elsewhere.dispatchEvent(tab)
    })
    fireEvent.keyDown(elsewhere, { key: 'e' })
    fireEvent.keyDown(elsewhere, { key: 'c' })

    expect(tab.defaultPrevented).toBe(false)
    const pills = within(screen.getByRole('radiogroup', { name: 'Marks here' }))
    expect(pills.getByRole('radio', { name: 'Confidence' }).getAttribute('aria-checked')).toBe('true')
    expect(screen.queryByRole('textbox', { name: 'What these words should say' })).toBeNull()
    expect(screen.queryByRole('textbox', { name: 'Comment' })).toBeNull()
  })

  it<Context>('takes focus from a press on a word, so its keys answer after a lane control had it', async (context) => {
    // jsdom has no pointer capture, which a press on a word takes to follow a drag.
    Object.assign(HTMLElement.prototype, {
      setPointerCapture: () => {},
      releasePointerCapture: () => {},
      hasPointerCapture: () => false,
    })
    renderWorkspace(context)
    const lane = screen.getByRole('switch', { name: 'Edit' })
    act(() => lane.focus())

    // A real press on a word: its pointerdown is canceled, so nothing moves focus but the page.
    fireEvent.pointerDown(wordPiece('Fryday.'), { pointerType: 'mouse', button: 0, pointerId: 1 })
    fireEvent.pointerUp(wordPiece('Fryday.'), { pointerType: 'mouse', button: 0, pointerId: 1 })
    expect(document.activeElement).toBe(screen.getByRole('document', { name: 'Transcript' }))

    press('e')
    expect(await screen.findByRole('textbox', { name: 'What these words should say' })).toBeTruthy()
  })

  it<Context>('copies the words selected only while the transcript has focus', (context) => {
    renderWorkspace(context)
    fireEvent.click(wordPiece('Launch'))
    fireEvent.click(wordPiece('is'), { shiftKey: true })

    const copied = (target: Element) => {
      const setData = vi.fn()
      const event = new Event('copy', { bubbles: true, cancelable: true })
      Object.assign(event, { clipboardData: { setData }})
      target.dispatchEvent(event)
      return setData.mock.calls[0]?.[1] ?? null
    }
    expect(copied(document.body)).toBe('Launch is')

    act(() => screen.getByRole('switch', { name: 'Edit' }).focus())
    expect(copied(document.body)).toBeNull()
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

  it<Context>('starts a thread on the words selected with C, mentioning whoever is picked', async (context) => {
    const change = { removedWordIds: [], words: [], marks: [ ...transcript.marks, mark('new', 'comment', 'w0', 'w1') ]}
    vi.mocked(startMeetingThread).mockResolvedValue({
      entry: entry('started', 'marked'),
      change,
      comment: { ...comment('first', 'new', '@Elysia who said this?'), mentions: [ ELYSIA_USER_ID ]},
    })
    renderWorkspace(context)

    fireEvent.click(wordPiece('Launch'))
    fireEvent.click(wordPiece('is'), { shiftKey: true })
    press('c')
    // The lane holds the composer; below `lg` it opens in the drawer, the copy drawn last.
    const field = (await screen.findAllByRole('textbox', { name: 'Comment' })).at(-1)
    if (!(field instanceof HTMLTextAreaElement)) {
      throw new Error('no comment field')
    }
    fireEvent.change(field, { target: { value: '@Ely' }})
    fireEvent.mouseDown(await screen.findByRole('option', { name: 'Elysia' }))
    expect(field.value).toBe('@Elysia ')
    fireEvent.change(field, { target: { value: '@Elysia who said this?' }})
    fireEvent.keyDown(field, { key: 'Enter', ctrlKey: true })

    await waitFor(() => expect(startMeetingThread).toHaveBeenCalledWith('meeting', {
      startWordId: 'w0',
      endWordId: 'w1',
      body: '@Elysia who said this?',
      mentions: [ ELYSIA_USER_ID ],
    }))
    expect(context.applyChange).toHaveBeenCalledWith(change)
    expect(vi.mocked(toast.success).mock.calls[0][0]).toBe('Comment added')
    expect(vi.mocked(toast.success).mock.calls[0][1]?.action?.label).toBe('Undo')
  })

  it<Context>('keeps whom a new thread mentions when its form is put away and opened again', async (context) => {
    vi.mocked(startMeetingThread).mockResolvedValue({
      entry: entry('started', 'marked'),
      change: { removedWordIds: [], words: [], marks: transcript.marks },
      comment: { ...comment('first', 'new', '@Elysia who?'), mentions: [ ELYSIA_USER_ID ]},
    })
    // A desktop, where the lane stands beside the transcript and no drawer opens over it.
    const matchMedia = window.matchMedia
    vi.spyOn(window, 'matchMedia').mockImplementation((query) => ({
      ...matchMedia(query),
      matches: query === '(min-width: 64rem)',
    }))
    renderWorkspace(context)
    const commentField = async () => {
      const field = (await screen.findAllByRole('textbox', { name: 'Comment' })).at(-1)
      if (!(field instanceof HTMLTextAreaElement)) {
        throw new Error('no comment field')
      }
      return field
    }

    fireEvent.click(wordPiece('Launch'))
    fireEvent.click(wordPiece('Launch'), { shiftKey: true })
    press('c')
    const first = await commentField()
    fireEvent.change(first, { target: { value: '@Ely' }})
    fireEvent.mouseDown(await screen.findByRole('option', { name: 'Elysia' }))
    fireEvent.change(first, { target: { value: '@Elysia who?' }})
    fireEvent.click(screen.getAllByRole('button', { name: 'Cancel' }).at(-1) ?? first)
    expect(screen.queryByRole('textbox', { name: 'Comment' })).toBeNull()

    // Opened again, it holds the text and the mention picked with it.
    fireEvent.click(wordPiece('Launch'))
    fireEvent.click(wordPiece('Launch'), { shiftKey: true })
    press('c')
    const again = await commentField()
    expect(again.value).toBe('@Elysia who?')
    fireEvent.keyDown(again, { key: 'Enter', ctrlKey: true })

    await waitFor(() => expect(startMeetingThread).toHaveBeenCalledWith('meeting', {
      startWordId: 'w0',
      endWordId: 'w0',
      body: '@Elysia who?',
      mentions: [ ELYSIA_USER_ID ],
    }))
  })

  it<Context>('opens a thread on its words, and resolves it with Undo', async (context) => {
    vi.mocked(getMeetingThreads).mockResolvedValue({ comments: [ comment('first', 'note', 'Is this Friday?') ]})
    const change = { removedWordIds: [], words: [], marks: [ mark('note', 'comment', 'w1', 'w2', { resolved: true }) ]}
    vi.mocked(setMeetingThreadResolved).mockResolvedValue({ entry: entry('resolved', 'thread_resolved'), change })
    renderWorkspace(context)

    // The comment is the only mark on this word, so a click opens its thread in the lane.
    fireEvent.click(wordPiece('is'))
    expect((await screen.findAllByText('Is this Friday?')).length).toBeGreaterThan(0)
    fireEvent.click(screen.getAllByRole('button', { name: 'Resolve' })[0])

    await waitFor(() => expect(setMeetingThreadResolved).toHaveBeenCalledWith('meeting', 'note', true))
    expect(context.applyChange).toHaveBeenCalledWith(change)
    expect(vi.mocked(toast.success).mock.calls[0][0]).toBe('Thread resolved')
  })

  it<Context>('opens a thread a link names, in the drawer where the lane is one', async (context) => {
    vi.mocked(getMeetingThreads).mockResolvedValue({ comments: [ comment('first', 'note', 'Is this Friday?') ]})
    // jsdom lays nothing out, so it has no scrolling to bring the thread's words into view.
    const scrollIntoView = vi.fn()
    Element.prototype.scrollIntoView = scrollIntoView
    try {
      renderWorkspace(context, transcript, '/?thread=note')

      // jsdom answers no media query, so this is a phone, whose lane is the drawer.
      const drawer = await screen.findByRole('dialog')
      expect((await within(drawer).findAllByText('Is this Friday?')).length).toBeGreaterThan(0)
      expect(wordPiece('is').hasAttribute('data-selected')).toBe(true)
      expect(scrollIntoView).toHaveBeenCalled()
    }
    finally {
      Reflect.deleteProperty(Element.prototype, 'scrollIntoView')
    }
  })

  it<Context>('draws a resolved thread no more, and lists it as resolved', async (context) => {
    vi.mocked(getMeetingThreads).mockResolvedValue({ comments: [ comment('first', 'note', 'Is this Friday?') ]})
    renderWorkspace(context, {
      ...transcript,
      marks: [ mark('note', 'comment', 'w1', 'w2', { resolved: true }) ],
    })

    expect(wordPiece('is').hasAttribute('data-comment-start')).toBe(false)
    expect((await screen.findAllByRole('button', { name: '1 resolved' })).length).toBeGreaterThan(0)
  })

  it<Context>('reads a meeting merged into another as it stood, offering nothing to change', async (context) => {
    vi.mocked(getMeetingThreads).mockResolvedValue({ comments: [ comment('first', 'note', 'Is this Friday?') ]})
    // A hold is undone wherever it is listed, but not on a meeting that changes elsewhere now.
    vi.mocked(getMeetingHistory).mockResolvedValue({ entries: [ entry('hold', 'hold_changed') ]})
    const mergedAway = makeMeeting({
      id: 'meeting',
      mergedInto: 'kept',
      // A corrected copy waiting for approval, which its own meeting would offer to approve.
      correction: {
        changedLines: 2,
        approvedAt: null,
        approvedBy: null,
        reviewItemId: null,
        createdAt: '2026-11-10T12:00:00Z',
      },
    })
    renderWorkspace(context, transcript, '/', { meeting: mergedAway })

    // Its actions say why, where Approve, holding, and adding a recording would be.
    expect(await screen.findByText('Edits, comments, and approval happen in the meeting it was merged into.'))
      .toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Approve' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Done reviewing' })).toBeNull()
    expect(screen.queryByText('Wait for my approval')).toBeNull()
    expect(screen.queryByText('Add a recording or transcript')).toBeNull()
    expect(await screen.findByText('Stopped holding the meeting for approval')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Undo' })).toBeNull()

    // Its words still select, and copy, but nothing edits them, starts a thread on them, takes them
    // into a project, or blacks them out.
    fireEvent.click(wordPiece('Launch'))
    fireEvent.click(wordPiece('is'), { shiftKey: true })
    const bar = screen.getByRole('toolbar', { name: 'What to do with the words selected' })
    for (const action of [ /Edit/, /Comment/, /Assign to a project/, /Black out/ ]) {
      expect(within(bar).queryByRole('button', { name: action })).toBeNull()
    }
    for (const key of [ 'e', 'c', 'p', 'b' ]) {
      press(key)
    }
    expect(screen.queryByRole('textbox', { name: 'What these words should say' })).toBeNull()
    expect(screen.queryByRole('textbox', { name: 'Comment' })).toBeNull()
    expect(screen.queryByRole('menu')).toBeNull()
    expect(blackOutMeetingWords).not.toHaveBeenCalled()

    // A thread reads alone, with no reply, Resolve, or Remove.
    press('Escape')
    fireEvent.click(wordPiece('is'))
    expect((await screen.findAllByText('Is this Friday?')).length).toBeGreaterThan(0)
    expect(screen.queryByRole('button', { name: 'Resolve' })).toBeNull()
    expect(screen.queryByRole('textbox', { name: 'Reply' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Remove thread' })).toBeNull()
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
  it<Context>('blacks out the words selected with B, applying what the route answered', async (context) => {
    vi.mocked(blackOutMeetingWords).mockResolvedValue({
      entry: { ...entry('blacked', 'marked'), data: { kind: 'black_out', absorbed: []}},
      change: { removedWordIds: [], words: [], marks: []},
    })
    renderWorkspace(context)

    fireEvent.click(wordPiece('Launch'))
    fireEvent.click(wordPiece('is'), { shiftKey: true })
    press('b')

    await waitFor(() => expect(blackOutMeetingWords).toHaveBeenCalledWith('meeting', {
      startWordId: 'w0',
      endWordId: 'w1',
    }))
    expect(context.applyChange).toHaveBeenCalledWith({ removedWordIds: [], words: [], marks: []})
    expect(vi.mocked(toast.success).mock.calls[0][0]).toBe('Blacked out')
    expect(vi.mocked(toast.success).mock.calls[0][1]?.action?.label).toBe('Undo')
  })

  it<Context>('takes the words selected into the project chosen from Assign to a project', async (context) => {
    vi.mocked(assignMeetingExcerpt).mockResolvedValue({
      entry: { ...entry('assigned', 'marked'), data: { kind: 'excerpt', recordId: 'record' }},
      change: { removedWordIds: [], words: [], marks: []},
      recordId: 'record',
    })
    renderWorkspace(context, transcript, '/', { projects: [ makeProject({ id: 'launch', name: 'Launch site' }) ]})

    fireEvent.click(wordPiece('Launch'))
    fireEvent.click(wordPiece('is'), { shiftKey: true })
    press('p')
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Launch site' }))

    await waitFor(() => expect(assignMeetingExcerpt).toHaveBeenCalledWith('meeting', {
      startWordId: 'w0',
      endWordId: 'w1',
      projectIds: [ 'launch' ],
    }))
    expect(vi.mocked(toast.success).mock.calls[0][0]).toBe('Taken into Launch site')
  })

  it<Context>('opens an excerpt with its title and projects, and removes it', async (context) => {
    vi.mocked(getTimelineRecord).mockResolvedValue({ record: excerptRecord([ 'launch' ]) })
    vi.mocked(removeMeetingMark).mockResolvedValue({
      entry: { ...entry('removed', 'unmarked'), data: { kind: 'excerpt', recordId: 'record' }},
      change: { removedWordIds: [], words: [], marks: []},
    })
    const excerpted: MeetingWords = {
      ...transcript,
      marks: [{
        ...mark('launch', 'excerpt', 'w0', 'w1'),
        excerpt: { recordId: 'record', title: 'The launch moves', projectIds: [ 'launch' ]},
      }],
    }
    renderWorkspace(context, excerpted, '/', { projects: [ makeProject({ id: 'launch', name: 'Launch site' }) ]})

    fireEvent.click(wordPiece('Launch'))

    expect(screen.getByText('The launch moves')).toBeTruthy()
    // Its bar in the gutter is named by its projects.
    expect(screen.getByRole('button', { name: 'Launch site' })).toBeTruthy()
    // Its record, read as it opens, shows the projects it is on.
    await waitFor(() => expect(getTimelineRecord).toHaveBeenCalledWith('record'))
    const detail = screen.getByRole('region', { name: 'Excerpt' })
    expect(await within(detail).findByText('Launch site')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Remove excerpt' }))
    await waitFor(() => expect(removeMeetingMark).toHaveBeenCalledWith('meeting', 'launch'))
    expect(vi.mocked(toast.success).mock.calls[0][0]).toBe('Excerpt removed')
  })

  it<Context>('shows a reader whose record it is not the projects an excerpt is on, reading no record', (context) => {
    const excerpted: MeetingWords = {
      ...transcript,
      marks: [{
        ...mark('launch', 'excerpt', 'w0', 'w1'),
        excerpt: { recordId: 'record', title: 'The launch moves', projectIds: [ 'launch' ]},
      }],
    }
    renderWorkspace(context, excerpted, '/', {
      projects: [ makeProject({ id: 'launch', name: 'Launch site' }) ],
      readerId: 'another-member',
    })

    fireEvent.click(wordPiece('Launch'))

    const detail = screen.getByRole('region', { name: 'Excerpt' })
    expect(within(within(detail).getByRole('list', { name: 'Its projects' })).getByText('Launch site')).toBeTruthy()
    expect(getTimelineRecord).not.toHaveBeenCalled()
  })

  it<Context>('names an excerpt\'s bar by its record\'s projects once its lane holds the record', async (context) => {
    // The words were read before the excerpt moved from Launch site to Billing; its record knows.
    vi.mocked(getTimelineRecord).mockResolvedValue({ record: excerptRecord([ 'billing' ]) })
    const excerpted: MeetingWords = {
      ...transcript,
      marks: [{
        ...mark('launch', 'excerpt', 'w0', 'w1'),
        excerpt: { recordId: 'record', title: 'The launch moves', projectIds: [ 'launch' ]},
      }],
    }
    renderWorkspace(context, excerpted, '/', {
      projects: [
        makeProject({ id: 'launch', name: 'Launch site' }),
        makeProject({ id: 'billing', name: 'Billing' }),
      ],
    })

    expect(screen.getByRole('button', { name: 'Launch site' })).toBeTruthy()
    fireEvent.click(wordPiece('Launch'))

    await waitFor(() => expect(screen.queryByRole('button', { name: 'Launch site' })).toBeNull())
    expect(screen.getAllByText('Billing').length).toBeGreaterThan(0)
  })

  it<Context>('opens the mark a link names, such as an excerpt from a project\'s timeline', async (context) => {
    vi.mocked(getTimelineRecord).mockResolvedValue({ record: excerptRecord([]) })
    const excerpted: MeetingWords = {
      ...transcript,
      marks: [{
        ...mark('launch', 'excerpt', 'w0', 'w1'),
        excerpt: { recordId: 'record', title: 'The launch moves', projectIds: []},
      }],
    }
    // jsdom lays nothing out, so it has no scrolling to bring the excerpt's words into view.
    Element.prototype.scrollIntoView = vi.fn()
    try {
      renderWorkspace(context, excerpted, '/?mark=launch')

      expect(wordPiece('Launch').hasAttribute('data-selected')).toBe(true)
      // jsdom answers no media query, so this is a phone, whose lane is the drawer.
      const drawer = await screen.findByRole('dialog')
      expect(within(drawer).getByText('The launch moves')).toBeTruthy()
    }
    finally {
      Reflect.deleteProperty(Element.prototype, 'scrollIntoView')
    }
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
      offsetSource: 'audio',
      hasRecording: true,
      recordingContentType: 'audio/mpeg',
      mergedFrom: null,
    },
    {
      id: 'video',
      connectorId: null,
      sourceKind: 'fireflies',
      url: null,
      offsetMs: 0,
      offsetSource: 'listed',
      hasRecording: true,
      recordingContentType: 'video/mp4',
      mergedFrom: null,
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

  it<Context>('plays and pauses on Space, and jumps ten seconds on the arrows, once it has focus', (context) => {
    renderWorkspace(context, recorded)
    const media = playerMedia()

    // Before the transcript or the player is pressed, Space and the arrows are the page's.
    expect(fireEvent.keyDown(document.body, { key: ' ' })).toBe(true)
    expect(HTMLMediaElement.prototype.play).not.toHaveBeenCalled()

    // A mouse press on the player's own controls hands focus to the player around them.
    fireEvent.focus(media)
    expect(document.activeElement).toBe(screen.getByRole('region', { name: 'Meeting recording' }))

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

  it<Context>('floats the same player once its place scrolls up out of sight, and docks as it returns', (context) => {
    type Seen = {
      isIntersecting: boolean
      boundingClientRect: { top: number }
      rootBounds: { bottom: number } | null
    }
    let report: (entries: Seen[]) => void = () => {}
    vi.stubGlobal('IntersectionObserver', class {
      constructor(callback: (entries: Seen[]) => void) {
        report = callback
      }
      observe() {}
      disconnect() {}
    })
    renderWorkspace(context, recorded)
    const media = playerMedia()
    const region = screen.getByRole('region', { name: 'Meeting recording' })

    act(() => report([{ isIntersecting: false, boundingClientRect: { top: -400 }, rootBounds: { bottom: 800 }}]))
    expect(region.className).toContain('fixed')
    expect(playerMedia()).toBe(media)
    expect(document.querySelector('[title="Drag to move the player"]')).toBeTruthy()

    act(() => report([{ isIntersecting: true, boundingClientRect: { top: 100 }, rootBounds: { bottom: 800 }}]))
    expect(region.className).not.toContain('fixed')
    expect(playerMedia()).toBe(media)
  })

  // A meeting opened while its recording is still being fetched: `meeting.changed` reads the
  // captures again once it is stored, and the player that then appears must watch its place.
  it<Context>('watches its place once a recording arrives in a meeting already open, and floats', (context) => {
    type Seen = {
      isIntersecting: boolean
      boundingClientRect: { top: number }
      rootBounds: { bottom: number } | null
    }
    const watched: Element[] = []
    let report: (entries: Seen[]) => void = () => {}
    vi.stubGlobal('IntersectionObserver', class {
      constructor(callback: (entries: Seen[]) => void) {
        report = callback
      }
      observe(element: Element) {
        watched.push(element)
      }
      disconnect() {}
    })
    const fetching: MeetingWords = {
      ...recorded,
      captures: recorded.captures.map((capture) => ({ ...capture, hasRecording: false, recordingContentType: null })),
    }
    const show = renderWorkspace(context, fetching)
    expect(document.querySelector('video')).toBeNull()

    show(recorded)

    expect(watched).toHaveLength(1)
    act(() => report([{ isIntersecting: false, boundingClientRect: { top: -400 }, rootBounds: { bottom: 800 }}]))
    expect(screen.getByRole('region', { name: 'Meeting recording' }).className).toContain('fixed')
  })

  it<Context>('listens to audio alone only once it plays, and closes its graph as the page goes', (context) => {
    const contexts: { resume: Mock, close: Mock }[] = []
    const node = { connect: vi.fn(), fftSize: 0 }
    vi.stubGlobal('AudioContext', class {
      resume = vi.fn().mockResolvedValue(undefined)
      close = vi.fn().mockResolvedValue(undefined)
      destination = {}
      constructor() {
        contexts.push(this)
      }
      createAnalyser() {
        return node
      }
      createMediaElementSource() {
        return node
      }
    })
    const audioOnly: MeetingWords = { ...recorded, captures: [ recorded.captures[0] ]}
    renderWorkspace(context, audioOnly)
    const media = playerMedia()

    // Loaded and seeking, nothing is routed through Web Audio, so nothing can be left suspended.
    fireEvent.loadedMetadata(media)
    expect(contexts).toHaveLength(0)

    fireEvent.play(media)
    expect(contexts).toHaveLength(1)
    expect(contexts[0].resume).toHaveBeenCalledTimes(1)

    // Played again, the same graph is resumed rather than another made.
    fireEvent.pause(media)
    fireEvent.play(media)
    expect(contexts).toHaveLength(1)
    expect(contexts[0].resume).toHaveBeenCalledTimes(2)

    cleanup()
    expect(contexts[0].close).toHaveBeenCalledTimes(1)
  })
})
