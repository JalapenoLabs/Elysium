// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { Mock } from 'vitest'
import type { MeetingHistoryEntry } from '../../../api/routes/meetingRoutes'

// Core
import { act, renderHook } from '@testing-library/react'
import { HTTPError } from 'ky'
import { Provider } from 'react-redux'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'
import { selectMeetingById } from '../../../store/meetingsSlice'

// Misc
import { setCaptureOffset, splitCapture, undoMeetingChange } from '../../../api/routes/meetingRoutes'
import { makeMeeting } from '../../../testFixtures'
import { toast } from '../../../toast/toast'
import { useMeetingChanges } from './useMeetingChanges'

vi.mock('../../../api/routes/meetingRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/meetingRoutes')>(),
  setCaptureOffset: vi.fn(),
  splitCapture: vi.fn(),
  undoMeetingChange: vi.fn(),
}))

vi.mock('../../../toast/toast', () => ({
  toast: Object.assign(vi.fn(), { success: vi.fn(), warning: vi.fn(), danger: vi.fn() }),
}))

type Context = {
  store: ReturnType<typeof createAppStore>
  reload: Mock<() => void>
}

function entry(id: string, kind: string, undid: string | null = null): MeetingHistoryEntry {
  return {
    id,
    kind,
    createdBy: 'person',
    agent: null,
    data: { from: 'other', into: 'meeting' },
    undid,
    createdAt: '2026-11-10T12:00:00Z',
  }
}

// A refusal as the API client hands one on, its body read into `data` (`src/api/index.ts`).
function refusal(status: number, code: string) {
  const error = new HTTPError(new Response(null, { status }), new Request('http://localhost/'), {} as never)
  error.data = { code, message: code }
  return error
}

function renderChanges(context: Context) {
  return renderHook(() => useMeetingChanges({
    meetingId: 'meeting',
    applyChange: () => {},
    reload: context.reload,
  }), {
    wrapper: (props: { children: ReactNode }) => <Provider store={context.store}>{props.children}</Provider>,
  })
}

// The Undo a change's toast offered.
function undoInToast() {
  const [ , options ] = vi.mocked(toast.success).mock.calls.at(-1) ?? []
  const action = options?.action
  if (!action) {
    throw new Error('the toast offered no Undo')
  }
  return action
}

describe('useMeetingChanges', () => {
  beforeEach<Context>((context) => {
    context.store = createAppStore()
    context.reload = vi.fn()
    vi.mocked(setCaptureOffset).mockReset()
    vi.mocked(splitCapture).mockReset()
    vi.mocked(undoMeetingChange).mockReset()
    vi.mocked(toast.success).mockReset()
    vi.mocked(toast.warning).mockReset()
    vi.mocked(toast.danger).mockReset()
    vi.mocked(undoMeetingChange).mockResolvedValue({
      entry: entry('undo', 'offset_changed', 'moved'),
      change: { removedWordIds: [], words: [], marks: []},
    })
  })

  it<Context>('moves a capture with Undo, leaving the transcript\'s reading to its announcement', async (context) => {
    vi.mocked(setCaptureOffset).mockResolvedValue({
      entry: entry('moved', 'offset_changed'),
      capture: { id: 'capture', offsetMs: 1_500, offsetSource: 'hand' },
    })
    const { result } = renderChanges(context)

    let moved = false
    await act(async () => {
      moved = await result.current.moveCapture('capture', 1_500)
    })

    expect(moved).toBe(true)
    expect(setCaptureOffset).toHaveBeenCalledWith('meeting', 'capture', 1_500)
    expect(vi.mocked(toast.success).mock.calls[0][0]).toBe('Recording moved on the meeting\'s clock')
    expect(context.reload).not.toHaveBeenCalled()

    await act(async () => undoInToast().onPress())
    expect(undoMeetingChange).toHaveBeenCalledWith('meeting', 'moved')
  })

  it<Context>('splits a capture out, takes the meeting answered, and Ctrl+Z merges it again', async (context) => {
    vi.mocked(splitCapture).mockResolvedValue({
      entry: entry('split', 'split', 'merged'),
      meeting: makeMeeting({ id: 'meeting', runId: 'alignment-run' }),
    })
    const { result } = renderChanges(context)

    await act(async () => result.current.splitOut('capture'))

    expect(splitCapture).toHaveBeenCalledWith('meeting', 'capture')
    expect(selectMeetingById(context.store.getState(), 'meeting')?.runId).toBe('alignment-run')
    expect(vi.mocked(toast.success).mock.calls[0][0]).toBe('Split back out into its own meeting')

    await act(async () => result.current.undoLast())
    expect(undoMeetingChange).toHaveBeenCalledWith('meeting', 'split')
  })

  it<Context>('reads the meeting again when a split finds it moved on, and says why', async (context) => {
    vi.mocked(splitCapture).mockRejectedValue(refusal(409, 'meeting_changed'))
    const { result } = renderChanges(context)

    await act(async () => result.current.splitOut('capture'))

    expect(context.reload).toHaveBeenCalled()
    expect(toast.warning).toHaveBeenCalledWith(
      'A recording from that meeting was moved by hand. Undo that move, then split it out.',
    )
    expect(toast.success).not.toHaveBeenCalled()
  })

  it<Context>('keeps a refused move out of what Ctrl+Z undoes', async (context) => {
    vi.mocked(setCaptureOffset).mockRejectedValue(refusal(400, 'offset_out_of_range'))
    const { result } = renderChanges(context)

    let moved = true
    await act(async () => {
      moved = await result.current.moveCapture('capture', 99_999_999)
    })
    await act(async () => result.current.undoLast())

    expect(moved).toBe(false)
    expect(toast.danger).toHaveBeenCalled()
    expect(undoMeetingChange).not.toHaveBeenCalled()
  })
})
