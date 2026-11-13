// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// The event stream reads which build the API runs when it starts and again on every
// reconnection's `hello`, since a new build only ever runs in a new API process, which drops
// the stream. Each test loads fresh modules, so a fresh store and stream.

// Stands in for the browser's EventSource, so a test decides when the stream opens and drops.
class FakeEventSource extends EventTarget {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSED = 2
  static created: FakeEventSource[] = []

  readyState = FakeEventSource.CONNECTING

  constructor() {
    super()
    FakeEventSource.created.push(this)
  }

  close() {
    this.readyState = FakeEventSource.CLOSED
  }

  open() {
    this.readyState = FakeEventSource.OPEN
    this.dispatchEvent(new Event('open'))
  }

  // A network error, which the browser retries by itself on the same EventSource.
  drop() {
    this.readyState = FakeEventSource.CONNECTING
    this.dispatchEvent(new Event('error'))
  }

  send(envelope: object) {
    this.dispatchEvent(new MessageEvent('message', { data: JSON.stringify(envelope) }))
  }
}

// jsdom has no fetch of its own, so ky builds Node's `Request`, which cannot resolve the
// origin-relative paths the app asks for. This one resolves them against the page.
class PageRequest extends Request {
  constructor(input: RequestInfo | URL, init?: RequestInit) {
    super(typeof input === 'string'
      ? new URL(input, window.location.href)
      : input, init)
  }
}

type Context = {
  // The build the stubbed API reports; a test changes it to play a release landing.
  runningBuild: string
  versionReads: number
  eventStream: typeof import('./eventStream')
  store: typeof import('../store').store
  selectIsRefreshOffered: typeof import('../store/versionSlice').selectIsRefreshOffered
}

function stream() {
  expect(FakeEventSource.created).toHaveLength(1)
  return FakeEventSource.created[0]
}

describe('the running build', () => {
  beforeEach<Context>(async (context) => {
    vi.resetModules()
    FakeEventSource.created = []
    vi.stubGlobal('EventSource', FakeEventSource)
    vi.stubGlobal('Request', PageRequest)

    context.runningBuild = '2026-11-04T00:00:00Z'
    context.versionReads = 0
    vi.stubGlobal('fetch', async (request: Request) => {
      if (new URL(request.url).pathname !== '/api/version') {
        return new Response('{}', { status: 404, headers: { 'content-type': 'application/json' }})
      }
      context.versionReads += 1
      return new Response(JSON.stringify({ builtAt: context.runningBuild }), {
        headers: { 'content-type': 'application/json' },
      })
    })

    context.eventStream = await import('./eventStream')
    context.store = (await import('../store')).store
    context.selectIsRefreshOffered = (await import('../store/versionSlice')).selectIsRefreshOffered
  })

  afterEach<Context>((context) => {
    context.eventStream.stopEventStream()
    vi.unstubAllGlobals()
  })

  it<Context>('is read once as the stream starts, and not again on its first hello', async (context) => {
    context.eventStream.startEventStream()
    await vi.waitFor(() => expect(context.store.getState().version.loadedBuild).toBe(context.runningBuild))

    stream().open()
    stream().send({ type: 'hello' })
    await new Promise((resolve) => setTimeout(resolve, 20))

    expect(context.versionReads).toBe(1)
    expect(context.selectIsRefreshOffered(context.store.getState())).toBe(false)
  })

  it<Context>('is read again when a reconnection says hello, offering a refresh for a new build', async (context) => {
    context.eventStream.startEventStream()
    await vi.waitFor(() => expect(context.versionReads).toBe(1))
    stream().open()
    stream().send({ type: 'hello' })

    // A release lands: the API restarts on the new build, and the stream reconnects to it.
    stream().drop()
    context.runningBuild = '2026-11-05T00:00:00Z'
    stream().open()
    stream().send({ type: 'hello' })

    await vi.waitFor(() => expect(context.selectIsRefreshOffered(context.store.getState())).toBe(true))
    expect(context.versionReads).toBe(2)
  })

  it<Context>('offers nothing when the API comes back on the same build', async (context) => {
    context.eventStream.startEventStream()
    await vi.waitFor(() => expect(context.versionReads).toBe(1))
    stream().open()
    stream().send({ type: 'hello' })

    stream().drop()
    stream().open()
    stream().send({ type: 'hello' })

    await vi.waitFor(() => expect(context.versionReads).toBe(2))
    expect(context.selectIsRefreshOffered(context.store.getState())).toBe(false)
  })
})

// A turn of Elysia's that read a meeting is announced by its id alone, since only the meeting's
// readers read what it says: each client loads it, and drops it when it is answered 404.
describe('a turn of Elysia\'s that read a meeting', () => {
  type TurnContext = {
    eventStream: typeof import('./eventStream')
    store: typeof import('../store').store
    elysiaSlice: typeof import('../store/elysiaSlice')
  }

  const readable = '01a1d60c-0000-7000-8000-000000000001'
  const heldBack = '01a1d60c-0000-7000-8000-000000000002'

  function turn(id: string, summary: string | null) {
    return {
      id,
      triggerKind: 'meeting-comment',
      triggerDetail: {},
      status: 'succeeded' as const,
      summary,
      error: null,
      createdBy: '0199a3c4-0000-7000-8000-00000000beef',
      createdAt: '2026-11-13T09:00:00Z',
      startedAt: '2026-11-13T09:00:01Z',
      finishedAt: '2026-11-13T09:00:30Z',
    }
  }

  beforeEach<TurnContext>(async (context) => {
    vi.resetModules()
    FakeEventSource.created = []
    vi.stubGlobal('EventSource', FakeEventSource)
    vi.stubGlobal('Request', PageRequest)
    vi.stubGlobal('fetch', async (request: Request) => {
      const headers = { 'content-type': 'application/json' }
      const path = new URL(request.url).pathname
      if (path === '/api/version') {
        return new Response(JSON.stringify({ builtAt: '2026-11-04T00:00:00Z' }), { headers })
      }
      if (path === `/api/v1/elysia/turns/${readable}`) {
        const body = { turn: turn(readable, 'Answered in the thread.') }
        return new Response(JSON.stringify(body), { headers })
      }
      return new Response('{}', { status: 404, headers })
    })

    context.eventStream = await import('./eventStream')
    context.store = (await import('../store')).store
    context.elysiaSlice = await import('../store/elysiaSlice')
  })

  afterEach<TurnContext>((context) => {
    context.eventStream.stopEventStream()
    vi.unstubAllGlobals()
  })

  it<TurnContext>('is loaded by its id for someone who may read it', async (context) => {
    context.store.dispatch(context.elysiaSlice.elysiaTurnUpserted(turn(readable, null)))
    context.eventStream.startEventStream()
    stream().open()
    stream().send({ type: 'hello' })
    stream().send({ type: 'elysiaTurn.changed', data: { id: readable }})

    await vi.waitFor(() => {
      const turns = context.elysiaSlice.selectElysiaTurns(context.store.getState())
      const loaded = turns.find((candidate) => candidate.id === readable)
      expect(loaded?.summary).toBe('Answered in the thread.')
    })
  })

  it<TurnContext>('is dropped for someone it is answered 404', async (context) => {
    context.store.dispatch(context.elysiaSlice.elysiaTurnUpserted(turn(heldBack, null)))
    context.eventStream.startEventStream()
    stream().open()
    stream().send({ type: 'hello' })
    stream().send({ type: 'elysiaTurn.changed', data: { id: heldBack }})

    await vi.waitFor(() => {
      const turns = context.elysiaSlice.selectElysiaTurns(context.store.getState())
      expect(turns.some((candidate) => candidate.id === heldBack)).toBe(false)
    })
  })
})

describe('a session turn that read a meeting', () => {
  type SessionContext = {
    eventStream: typeof import('./eventStream')
    store: typeof import('../store').store
    sessionEventsSlice: typeof import('../store/sessionEventsSlice')
    // The sequence each read of the conversation's history went on from, in order.
    tailReads: string[]
  }

  const turnId = '01a1d60c-0000-7000-8000-000000000003'

  function event(sequence: number) {
    return {
      sessionId: 7,
      sequence,
      turnId,
      occurredAt: '2026-11-13T09:00:00Z',
      type: 'agent.message',
      payload: { kind: 'agentMessage' as const, text: `Said ${sequence}.` },
    }
  }

  beforeEach<SessionContext>(async (context) => {
    vi.resetModules()
    vi.useFakeTimers()
    FakeEventSource.created = []
    context.tailReads = []
    vi.stubGlobal('EventSource', FakeEventSource)
    vi.stubGlobal('Request', PageRequest)
    vi.stubGlobal('fetch', async (request: Request) => {
      const headers = { 'content-type': 'application/json' }
      const url = new URL(request.url)
      if (url.pathname === '/api/version') {
        return new Response(JSON.stringify({ builtAt: '2026-11-04T00:00:00Z' }), { headers })
      }
      if (url.pathname === '/api/v1/coding-sessions/7/events') {
        const after = Number(url.searchParams.get('afterSequence'))
        context.tailReads.push(String(after))
        const body = {
          events: [ event(after + 1) ],
          truncated: false,
          summaries: [{ sessionId: 7, turnId, summary: 'Read the move.', createdAt: '2026-11-13T09:01:00Z' }],
        }
        return new Response(JSON.stringify(body), { headers })
      }
      return new Response('{}', { status: 404, headers })
    })

    context.eventStream = await import('./eventStream')
    context.store = (await import('../store')).store
    context.sessionEventsSlice = await import('../store/sessionEventsSlice')
  })

  afterEach<SessionContext>((context) => {
    context.eventStream.stopEventStream()
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  it<SessionContext>('catches an open conversation up once a burst, reading only what follows', async (context) => {
    const { sessionHistoryLoaded, sessionTimelineOpened, selectSessionTimeline } = context.sessionEventsSlice
    context.store.dispatch(sessionTimelineOpened(7))
    const history = { events: [ event(3) ], truncated: false, summaries: []}
    context.store.dispatch(sessionHistoryLoaded({ sessionId: 7, history }))
    context.eventStream.startEventStream()
    stream().open()
    stream().send({ type: 'hello' })
    for (const sequence of [ 4, 5, 6 ]) {
      stream().send({ type: 'session.eventHeld', data: { sessionId: 7, sequence }})
    }
    stream().send({ type: 'session.summaryChanged', data: { sessionId: 7, turnId }})
    expect(context.tailReads).toEqual([])

    await vi.advanceTimersByTimeAsync(1_000)
    await vi.waitFor(() => {
      const timeline = selectSessionTimeline(context.store.getState(), 7)
      expect(timeline?.events.map((held) => held.sequence)).toEqual([ 3, 4 ])
      expect(timeline?.summariesByTurnId[turnId]).toBe('Read the move.')
    })
    expect(context.tailReads).toEqual([ '3' ])

    stream().send({ type: 'session.eventHeld', data: { sessionId: 7, sequence: 7 }})
    await vi.advanceTimersByTimeAsync(1_000)
    await vi.waitFor(() => expect(context.tailReads).toEqual([ '3', '4' ]))
    expect(selectSessionTimeline(context.store.getState(), 7)?.truncated).toBe(false)
  })

  it<SessionContext>('reads nothing for a conversation nobody has open', async (context) => {
    context.eventStream.startEventStream()
    stream().open()
    stream().send({ type: 'hello' })
    stream().send({ type: 'session.eventHeld', data: { sessionId: 7, sequence: 4 }})

    await vi.advanceTimersByTimeAsync(1_000)
    expect(context.tailReads).toEqual([])
  })
})
