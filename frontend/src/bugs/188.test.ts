// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Issue #188: a session signed out, or ended from the Sessions tab, kept working for as long as
// the API had cached it, because the browser asked Kratos directly and the API never heard. Now
// the browser asks the API, which ends the session with Kratos and forgets it at once, and
// nginx no longer lets a browser end one through Kratos. These hold the frontend to that.

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
  // Every request the page made, as `METHOD /path`.
  requests: string[]
  // The status the stubbed API answers with.
  status: number
  assign: ReturnType<typeof vi.fn<(url: string | URL) => void>>
}

describe('ending a session', () => {
  beforeEach<Context>((context) => {
    vi.resetModules()
    context.requests = []
    context.status = 204
    vi.stubGlobal('Request', PageRequest)
    vi.stubGlobal('fetch', async (request: Request) => {
      context.requests.push(`${request.method} ${new URL(request.url).pathname}`)
      return new Response(null, { status: context.status })
    })
    context.assign = vi.fn<(url: string | URL) => void>()
    vi.spyOn(window, 'location', 'get').mockReturnValue({
      ...window.location,
      href: window.location.href,
      assign: context.assign,
    })
  })

  afterEach(() => {
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  it<Context>('signs out through the API, then opens the sign-in page', async (context) => {
    const { signOut } = await import('../auth/signOut')
    const { UrlTree } = await import('../urls')

    await signOut()

    expect(context.requests).toEqual([ 'POST /api/v1/auth/sign-out' ])
    expect(context.assign).toHaveBeenCalledWith(UrlTree.login)
  })

  it<Context>('opens the sign-in page even when signing out fails', async (context) => {
    context.status = 502
    const { signOut } = await import('../auth/signOut')
    const { UrlTree } = await import('../urls')

    await signOut()

    expect(context.assign).toHaveBeenCalledWith(UrlTree.login)
  })

  it<Context>('ends other sessions through the API, never through Kratos', async (context) => {
    const { endOtherSessions, endSession } = await import('../api/routes/authRoutes')

    await endSession('0199a3c4-0000-7000-8000-00000000beef')
    await endOtherSessions()

    expect(context.requests).toEqual([
      'DELETE /api/v1/me/sessions/0199a3c4-0000-7000-8000-00000000beef',
      'DELETE /api/v1/me/sessions',
    ])
  })
})
