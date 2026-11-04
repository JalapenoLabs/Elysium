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
