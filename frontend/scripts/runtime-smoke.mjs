// Copyright © 2026 Jalapeno Labs

// Runtime smoke check: proves the production bundle actually boots in a real
// browser. It serves the built `dist/` through Vite's preview server, opens it
// in headless Chromium, and fails on any page error or console error. The API
// is stubbed at the network layer (the session probe answers 401, the normal
// anonymous path), so the check is hermetic: no backend, no Keycloak, no
// network access required.
//
// Run from the frontend directory, after `yarn build`:
//   node scripts/runtime-smoke.mjs

import { preview } from 'vite'
import { chromium } from 'playwright'

// The signed-out boot path: the SPA probes /api/v1/me on load, treats the 401
// as "anonymous", and redirects to the sign-in screen. Waiting for the sign-in
// heading proves the whole chain ran: bundle parsed, i18n bundles fetched,
// session resolved, router redirected, and React painted the signed-out view.
const SESSION_PROBE_PATH = '/api/v1/me'
const SIGN_IN_HEADING = 'h1:has-text("Welcome back")'

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
    // The stubbed 401 on the session probe makes Chromium log
    // "Failed to load resource ... 401". That is the expected anonymous
    // path, not an app failure, so it is the one error we tolerate.
    if (url.includes(SESSION_PROBE_PATH) && text.includes('401')) {
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

    if (new URL(request.url()).pathname === SESSION_PROBE_PATH) {
      // 401 is the normal anonymous answer; the SPA treats it as signed-out.
      await route.fulfill({
        status: 401,
        headers: corsHeaders,
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
  await page.waitForSelector(SIGN_IN_HEADING, { timeout: 15_000 })

  if (!page.url().includes('/sign-in')) {
    throw new Error(`Expected the anonymous boot to land on /sign-in, but ended at ${page.url()}`)
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
