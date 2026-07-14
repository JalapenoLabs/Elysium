// Copyright © 2026 Jalapeno Labs

// Runtime smoke check: proves the production bundle actually boots in a real
// browser. It serves the built `dist/` through Vite's preview server, opens it
// in headless Chromium, and fails on any page error or console error. The API
// is stubbed at the network layer (the session bootstrap answers 401, the
// normal anonymous path), so the check is hermetic: no backend and no network
// access required.
//
// Run from the frontend directory, after `yarn build`:
//   node scripts/runtime-smoke.mjs

import { preview } from 'vite'
import { chromium } from 'playwright'

// The signed-out boot path: the SPA connects its realtime stream, fetches the
// GET /api/v1/context bootstrap, treats the 401 as "anonymous" (tearing the
// stream back down), confirms the instance is claimed, and redirects to the
// login gate. Waiting for the gate's wordmark proves the whole chain ran:
// bundle parsed, i18n bundles fetched, session resolved, router redirected,
// and React painted the signed-out view.
const SESSION_BOOTSTRAP_PATH = '/api/v1/context'
const CLAIM_PROBE_PATH = '/api/v1/auth/context'
const EVENTS_PATH = '/api/v1/events'
const GATE_WORDMARK = '.login-wordmark:has-text("Elysium")'

const pageErrors = []
const consoleErrors = []

let server = null
let browser = null

try {
  server = await preview({
    preview: {
      // Port 0 asks the kernel for an ephemeral port, so parallel CI jobs
      // never collide; the real port is read back from the bound socket.
      port: 0,
      host: '127.0.0.1',
    },
  })

  const address = server.httpServer.address()
  if (address === null || typeof address === 'string') {
    throw new Error(`Expected an AddressInfo from the preview server, got: ${JSON.stringify(address)}`)
  }
  const baseUrl = `http://127.0.0.1:${address.port}`
  console.log(`Preview server bound to ${baseUrl}`)

  browser = await chromium.launch()
  const page = await browser.newPage()

  page.on('pageerror', (error) => {
    pageErrors.push(String(error))
  })
  page.on('console', (message) => {
    if (message.type() !== 'error') {
      return
    }
    const url = message.location().url || ''
    const text = message.text()
    // The stubbed 401s on the session bootstrap and the SSE endpoint make
    // Chromium log "Failed to load resource ... 401". That is the expected
    // anonymous path, not an app failure, so those are the errors we tolerate.
    if (url.includes('/api/v1/') && text.includes('401')) {
      return
    }
    consoleErrors.push(`${text} (${url})`)
  })

  // Stub the entire API surface so the check never needs a backend. The
  // fulfilled responses carry CORS headers because the app calls the API on
  // its own origin (http://localhost:31416) with credentials included.
  await page.route('**/api/**', async (route) => {
    const request = route.request()
    const corsHeaders = {
      'access-control-allow-origin': request.headers()['origin'] || '*',
      'access-control-allow-credentials': 'true',
    }

    const pathname = new URL(request.url()).pathname

    if (pathname === SESSION_BOOTSTRAP_PATH || pathname === EVENTS_PATH) {
      // 401 is the normal anonymous answer: the SPA treats the bootstrap 401
      // as signed-out and stops the realtime stream.
      await route.fulfill({
        status: 401,
        headers: corsHeaders,
      })
      return
    }

    if (pathname === CLAIM_PROBE_PATH) {
      // A claimed instance sends the anonymous visitor to /login (an
      // unclaimed one would route to first-run setup instead).
      await route.fulfill({
        status: 200,
        headers: { ...corsHeaders, 'content-type': 'application/json' },
        body: JSON.stringify({ claimed: true }),
      })
      return
    }

    await route.fulfill({
      status: 200,
      headers: corsHeaders,
      body: '',
    })
  })

  await page.goto(baseUrl, { waitUntil: 'load' })

  // The signed-out view must actually paint, not just leave #root non-empty.
  await page.waitForSelector(GATE_WORDMARK, { timeout: 15_000 })

  if (!page.url().includes('/login')) {
    throw new Error(`Expected the anonymous boot to land on /login, but ended at ${page.url()}`)
  }

  if (pageErrors.length || consoleErrors.length) {
    console.error('Runtime smoke check FAILED: the bundle booted with errors.')
    for (const error of pageErrors) {
      console.error(`  pageerror: ${error}`)
    }
    for (const error of consoleErrors) {
      console.error(`  console error: ${error}`)
    }
    process.exit(1)
  }

  console.log('Runtime smoke check passed: production bundle booted cleanly to the signed-out view.')
}
catch (error) {
  console.error('Runtime smoke check FAILED:', error)
  for (const pageError of pageErrors) {
    console.error(`  pageerror: ${pageError}`)
  }
  for (const consoleError of consoleErrors) {
    console.error(`  console error: ${consoleError}`)
  }
  process.exit(1)
}
finally {
  if (browser) {
    await browser.close()
  }
  if (server) {
    await server.close()
  }
}
