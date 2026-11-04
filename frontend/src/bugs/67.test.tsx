// Copyright © 2026 Jalapeno Labs

import type { Me } from '../api/routes/authRoutes'
import type { User } from '../api/routes/userRoutes'

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Issue #67: refreshing a workspace page sent every read twice. The workspace read its data while
// the event stream was still connecting, and the stream's `hello` then reloaded everything on
// screen, because it could not tell whether those reads predated its subscription. The stream
// now subscribes first: `AuthGate` holds the workspace until it has opened, so the first `hello`
// has nothing to reload, while a reconnection's `hello` still reloads what is shown.

// Each test is one page load: fresh modules, so a fresh Redux store, event stream, and SWR cache
// (SWR is inlined for tests in `vite.config.ts` so that `vi.resetModules` reaches its cache).

const now = '2026-11-03T20:00:00.000Z'

const person: User = {
  id: '00000000-0000-7000-8000-000000000001',
  kind: 'person',
  name: 'Alex Navarro',
  email: 'alex@example.com',
  role: 'admin',
  status: 'active',
  approvedAt: now,
  lastSeenAt: now,
  createdAt: now,
  updatedAt: now,
}

const me: Me = {
  user: person,
  hasSecondFactor: true,
  mfaEnrollmentRequired: false,
  authenticatorNickname: null,
}

// What the stubbed API answers, by path.
const answers: Record<string, unknown> = {
  '/api/v1/me': me,
  '/api/v1/users': { users: [ person ]},
}

// Stands in for the browser's EventSource, so a test decides when the stream opens, drops, and
// what it is sent.
class FakeEventSource extends EventTarget {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSED = 2
  static created: FakeEventSource[] = []

  readonly url: string
  readyState = FakeEventSource.CONNECTING

  constructor(url: string) {
    super()
    this.url = url
    FakeEventSource.created.push(this)
  }

  close() {
    this.readyState = FakeEventSource.CLOSED
  }

  // The server answered: it subscribed to the bus before it sent its headers.
  open() {
    this.readyState = FakeEventSource.OPEN
    this.dispatchEvent(new Event('open'))
  }

  // The connection dropped and the browser will retry it by itself, as it does for a network
  // error (as opposed to an HTTP error response, which closes it).
  drop() {
    this.readyState = FakeEventSource.CONNECTING
    this.dispatchEvent(new Event('error'))
  }

  send(envelope: object) {
    this.dispatchEvent(new MessageEvent('message', { data: JSON.stringify(envelope) }))
  }
}

// jsdom has no fetch of its own, so ky builds Node's `Request`, which cannot resolve the
// origin-relative paths the app asks for, as a browser's does. This one resolves them against
// the page.
class PageRequest extends Request {
  constructor(input: RequestInfo | URL, init?: RequestInit) {
    super(typeof input === 'string'
      ? new URL(input, window.location.href)
      : input, init)
  }
}

type Context = {
  requests: string[]
  stopEventStream: () => void
  unmount: () => void
  waitFor: (assertion: () => void) => Promise<void>
}

// How many times the API was asked for `path`.
function countOf(requests: string[], path: string) {
  return requests.filter((requested) => requested === path).length
}

// The one stream the page opened.
function stream() {
  expect(FakeEventSource.created).toHaveLength(1)
  return FakeEventSource.created[0]
}

describe('a page load', () => {
  beforeEach<Context>(async (context) => {
    vi.resetModules()
    FakeEventSource.created = []
    vi.stubGlobal('EventSource', FakeEventSource)

    context.requests = []
    vi.stubGlobal('Request', PageRequest)
    vi.stubGlobal('fetch', async (request: Request) => {
      const path = new URL(request.url).pathname
      context.requests.push(path)
      const answer = answers[path]
      return new Response(JSON.stringify(answer ?? { message: 'not stubbed' }), {
        status: answer
          ? 200
          : 404,
        headers: { 'content-type': 'application/json' },
      })
    })

    const { render, screen, waitFor } = await import('@testing-library/react')
    const { Provider } = await import('react-redux')
    const { createMemoryRouter, RouterProvider } = await import('react-router')
    const { SWRConfig } = await import('swr')
    const { store } = await import('../store')
    const { AuthGate } = await import('../auth/AuthGate')
    const { useUsersLoader } = await import('../hooks/useServerData')
    const { stopEventStream } = await import('../realtime/eventStream')

    // A workspace page that reads server data, as every page does.
    function UsersPage() {
      const status = useUsersLoader()
      return <p>{`users ${status}`}</p>
    }

    const router = createMemoryRouter(
      [{
        element: <AuthGate />,
        children: [{ path: '/settings', element: <UsersPage /> }],
      }],
      { initialEntries: [ '/settings' ]},
    )

    // The same SWR options the app runs with (`src/main.tsx`).
    const { unmount } = render(<Provider store={store}>
      <SWRConfig value={{ revalidateOnFocus: false, revalidateOnReconnect: false }}>
        <RouterProvider router={router} />
      </SWRConfig>
    </Provider>)

    context.stopEventStream = stopEventStream
    context.unmount = unmount
    context.waitFor = async (assertion) => {
      await waitFor(assertion)
    }

    // The person is signed in, so the stream starts; nothing is read until it opens.
    await waitFor(() => expect(FakeEventSource.created).toHaveLength(1))
    expect(screen.queryByText(/^users /)).toBeNull()
  })

  afterEach<Context>((context) => {
    context.stopEventStream()
    context.unmount()
    vi.unstubAllGlobals()
  })

  it<Context>('reads each thing once when the stream opens and says hello', async (context) => {
    stream().open()
    stream().send({ type: 'hello' })

    await context.waitFor(() => expect(countOf(context.requests, '/api/v1/users')).toBe(1))
    // Give a second read every chance to be sent before counting.
    await new Promise((resolve) => setTimeout(resolve, 50))

    expect(countOf(context.requests, '/api/v1/me')).toBe(1)
    expect(countOf(context.requests, '/api/v1/users')).toBe(1)
  })

  it<Context>('reloads what is on screen when a reconnection says hello', async (context) => {
    stream().open()
    stream().send({ type: 'hello' })
    await context.waitFor(() => expect(countOf(context.requests, '/api/v1/users')).toBe(1))

    stream().drop()
    stream().open()
    stream().send({ type: 'hello' })

    await context.waitFor(() => expect(countOf(context.requests, '/api/v1/users')).toBe(2))
    expect(countOf(context.requests, '/api/v1/me')).toBe(2)
  })

  it<Context>('still shows the workspace when the stream fails before it opens', async (context) => {
    stream().drop()

    await context.waitFor(() => expect(countOf(context.requests, '/api/v1/users')).toBe(1))

    // That read may predate the subscription, so the stream's first hello reloads it.
    stream().open()
    stream().send({ type: 'hello' })
    await context.waitFor(() => expect(countOf(context.requests, '/api/v1/users')).toBe(2))
  })
})
