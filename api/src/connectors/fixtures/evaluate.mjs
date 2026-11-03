// Copyright © 2026 Jalapeno Labs
//
// Evaluates a connector's page script in a fixture page, in headless Chromium, the way the API
// does over DevTools, and prints what it answered as JSON. A check for the scripts themselves,
// which run as the page and so cannot run under `cargo test`:
//
//   python3 -m http.server 8765 --bind 127.0.0.1 --directory api/src/connectors/fixtures
//   node api/src/connectors/fixtures/evaluate.mjs api/src/connectors/teams.js \
//     http://127.0.0.1:8765/teams.html '{"since":1730000000000,"maxMessages":200}'
//
// A script written as a function (teams.js) is applied to the arguments; one written as an
// expression (outlook.js) is evaluated as it is when no arguments are given. Needs `chromium`
// on the PATH and Node 24, for its global WebSocket.

import { spawn } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const [ scriptPath, pageUrl, argumentsJson ] = process.argv.slice(2)
if (!scriptPath || !pageUrl) {
  console.error('usage: evaluate.mjs <script> <fixture url> [arguments json]')
  process.exit(2)
}

const script = readFileSync(scriptPath, 'utf8')
const expression = argumentsJson
  ? `(${script})(${argumentsJson})`
  : script
const DEVTOOLS_PORT = 9333
const profile = mkdtempSync(join(tmpdir(), 'elysium-fixture-'))
const chromium = spawn('chromium', [
  '--headless=new',
  `--remote-debugging-port=${DEVTOOLS_PORT}`,
  `--user-data-dir=${profile}`,
  'about:blank',
], { stdio: 'ignore' })

const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds))

async function pageSocketUrl() {
  for (let attempt = 0; attempt < 50; attempt++) {
    try {
      const tabs = await (await fetch(`http://127.0.0.1:${DEVTOOLS_PORT}/json/list`)).json()
      return tabs.find((tab) => tab.type === 'page').webSocketDebuggerUrl
    }
    catch {
      await sleep(200)
    }
  }
  throw new Error('Chromium never opened its DevTools port')
}

try {
  const socket = new WebSocket(await pageSocketUrl())
  await new Promise((resolve) => socket.addEventListener('open', resolve))
  const pending = new Map()
  let nextId = 1
  socket.addEventListener('message', (event) => {
    const answer = JSON.parse(event.data)
    pending.get(answer.id)?.(answer)
    pending.delete(answer.id)
  })
  const call = (method, params) => new Promise((resolve) => {
    const id = nextId++
    pending.set(id, resolve)
    socket.send(JSON.stringify({ id, method, params }))
  })

  await call('Page.enable', {})
  await call('Page.navigate', { url: pageUrl })
  await sleep(1500)
  // A fixture that seeds a store sets `window.seeded` to a promise; anything else is ready.
  await call('Runtime.evaluate', { expression: 'window.seeded', awaitPromise: true })
  const answer = await call('Runtime.evaluate', {
    expression,
    awaitPromise: true,
    returnByValue: true,
  })
  socket.close()
  if (answer.result.exceptionDetails) {
    console.error(JSON.stringify(answer.result.exceptionDetails, null, 2))
    process.exitCode = 1
  }
  else {
    console.log(JSON.stringify(answer.result.result.value, null, 2))
  }
}
finally {
  chromium.kill()
  rmSync(profile, { recursive: true, force: true })
}
