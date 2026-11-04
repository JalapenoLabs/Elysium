// Copyright © 2026 Jalapeno Labs

import type { Input } from 'ky'

// Core
import i18next from 'i18next'

// Misc
import { SLOW_REQUEST_NOTICE_MS } from '../constants'
import { toast } from '../toast/toast'

// Reads fetch what a page shows, and many run unasked (a reconnected event stream revalidates
// every view), so only a change the person asked for is announced.
const unannouncedMethods = new Set([ 'GET', 'HEAD' ])

// One notice speaks for every slow change at once, so overlapping ones never stack copies of
// it. It closes when the last of them returns.
let slowRequestCount = 0
let noticeId: string | null = null

// The `fetch` the API client sends through. A change still running after
// `SLOW_REQUEST_NOTICE_MS` shows "taking longer than usual" until it returns, fails, or times
// out. Wrapping fetch itself, rather than ky's hooks, covers every way a request can end,
// including ky's own timeout aborting it, and each retry is timed on its own.
export async function fetchWithSlowRequestNotice(input: Input, init?: RequestInit) {
  const method = input instanceof Request
    ? input.method
    : init?.method ?? 'GET'
  if (unannouncedMethods.has(method.toUpperCase())) {
    return globalThis.fetch(input, init)
  }

  let isSlow = false
  const timer = setTimeout(() => {
    isSlow = true
    slowRequestCount += 1
    noticeId ??= toast.loading(i18next.t('common:requests.slow'))
  }, SLOW_REQUEST_NOTICE_MS)

  try {
    return await globalThis.fetch(input, init)
  }
  finally {
    clearTimeout(timer)
    if (isSlow) {
      slowRequestCount -= 1
    }
    if (isSlow && !slowRequestCount && noticeId) {
      toast.close(noticeId)
      noticeId = null
    }
  }
}
