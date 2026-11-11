// Copyright © 2026 Jalapeno Labs
//
// slack.js against a fixture of Slack's web client: its `localConfig_v2` session and a fake
// web API in place of `fetch`. Run with `node --test api/src/connectors/*.test.mjs`.

// Core
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { test } from 'node:test'
import vm from 'node:vm'

const source = await readFile(new URL('./slack.js', import.meta.url), 'utf8')

const MOORESLAB = {
  id: 'T1',
  name: 'Mooreslab',
  url: 'https://mooreslab.slack.com/',
  token: 'xoxc-mooreslab',
  user_id: 'U0ME',
}

// Slack's CORS rule for a workspace host, as the browser applies it: a credentialed call from
// app.slack.com is answered with `Access-Control-Allow-Origin: *` unless it carries the web
// client's `_x_gantry` marker, and the browser refuses `*` alongside cookies, which the page
// sees as `fetch` throwing.
function refuseAsSlackDoes(url, init) {
  const address = new URL(String(url))
  if (init.credentials === 'include' && address.searchParams.get('_x_gantry') !== 'true') {
    throw new TypeError('Failed to fetch')
  }
}

// Runs slack.js as the page would, with `request` in place of its argument. `answer` is the
// fake web API: given the address and form fields of a call, it answers Slack's JSON.
async function evaluate(request, { teams, answer = () => ({ ok: true }) }) {
  const calls = []
  const storage = {
    getItem: (key) => (key === 'localConfig_v2' && teams
      ? JSON.stringify({ teams })
      : null),
  }
  const fetch = async (url, init) => {
    refuseAsSlackDoes(url, init)
    const fields = Object.fromEntries(init.body.entries())
    calls.push({ url: String(url), fields, credentials: init.credentials })
    const body = await answer(String(url), fields)
    return { ok: true, status: 200, json: async () => body }
  }
  const script = source.replace('__ELYSIUM_REQUEST__', JSON.stringify(request))
  const answered = await vm.runInNewContext(script, { localStorage: storage, fetch, FormData, URL })
  // DevTools hands the API the value as JSON (`returnByValue`), which also leaves the script's
  // own context behind, so assertions compare plain values.
  const result = JSON.parse(JSON.stringify(answered))
  return { result, calls }
}

test('a tab with no signed-in workspace answers signed out and calls nothing', async () => {
  const { result, calls } = await evaluate({ counts: true, calls: [] }, { teams: null })
  assert.equal(result.signedIn, false)
  assert.deepEqual(calls, [])
})

test('counts are asked of every workspace, with its own session, at its own host', async () => {
  const jalapeno = { ...MOORESLAB, id: 'T2', name: 'Jalapeno', url: 'https://jalapeno.slack.com/', token: 'xoxc-j' }
  const { result, calls } = await evaluate({ counts: true, calls: [] }, {
    teams: { T1: MOORESLAB, T2: jalapeno },
    answer: (url) => ({ ok: true, channels: [], asked: url }),
  })

  assert.equal(result.signedIn, true)
  assert.deepEqual(result.teams.map((team) => [ team.id, team.userId, team.counts.asked ]), [
    [ 'T1', 'U0ME', 'https://mooreslab.slack.com/api/client.counts?_x_gantry=true&slack_route=T1' ],
    [ 'T2', 'U0ME', 'https://jalapeno.slack.com/api/client.counts?_x_gantry=true&slack_route=T2' ],
  ])
  assert.deepEqual(calls.map((call) => call.fields.token), [ 'xoxc-mooreslab', 'xoxc-j' ])
  assert.ok(calls.every((call) => call.credentials === 'include'))
})

test('the session token never leaves the page', async () => {
  const { result } = await evaluate({ counts: true, calls: [] }, { teams: { T1: MOORESLAB } })
  assert.ok(!JSON.stringify(result).includes('xoxc-'))
})

test('a workspace stored at a host that is not Slack is never called', async () => {
  const elsewhere = { ...MOORESLAB, id: 'T9', url: 'https://slack.com.example.org/' }
  const { result, calls } = await evaluate({ counts: true, calls: [] }, { teams: { T9: elsewhere } })
  assert.equal(result.signedIn, false)
  assert.deepEqual(calls, [])
})

test('calls carry their parameters, and only read methods are made', async () => {
  const { result, calls } = await evaluate({
    counts: false,
    calls: [
      { team: 'T1', method: 'conversations.history', params: { channel: 'C1', oldest: '1730000000.000100', limit: 100 } },
      { team: 'T1', method: 'chat.postMessage', params: { channel: 'C1', text: 'hi' } },
      { team: 'T1', method: 'conversations.mark', params: { channel: 'C1', ts: '1' } },
      { team: 'T7', method: 'users.info', params: { user: 'U1' } },
    ],
  }, { teams: { T1: MOORESLAB }, answer: () => ({ ok: true, messages: [] }) })

  assert.equal(calls.length, 1)
  assert.equal(
    calls[0].url,
    'https://mooreslab.slack.com/api/conversations.history?_x_gantry=true&slack_route=T1',
  )
  assert.deepEqual(calls[0].fields, { token: 'xoxc-mooreslab', channel: 'C1', oldest: '1730000000.000100', limit: '100' })
  assert.deepEqual(result.results, [
    { ok: true, messages: [] },
    { ok: false, error: 'method_not_allowed' },
    { ok: false, error: 'method_not_allowed' },
    { ok: false, error: 'team_not_signed_in' },
  ])
  assert.equal(result.teams[0].counts, null)
})

test('a call the network refuses is answered as a failure, not thrown', async () => {
  const { result } = await evaluate({
    counts: false,
    calls: [ { team: 'T1', method: 'auth.test', params: {} } ],
  }, {
    teams: { T1: MOORESLAB },
    answer: () => {
      throw new TypeError('Failed to fetch')
    },
  })
  assert.equal(result.results[0].ok, false)
  assert.equal(result.results[0].error, 'fetch_failed')
})

// Slack answered every credentialed call from app.slack.com with a wildcard origin, which the
// browser refuses, so client.counts read as fetch_failed for every workspace. The calls carry
// the web client's own marker, which Slack answers with app.slack.com's origin.
test('every call is one Slack answers a credentialed cross-origin request for', async () => {
  const { result, calls } = await evaluate({
    counts: true,
    calls: [ { team: 'T1', method: 'users.info', params: { user: 'U1' } } ],
  }, { teams: { T1: MOORESLAB }, answer: () => ({ ok: true, channels: [] }) })

  assert.equal(result.teams[0].counts.ok, true)
  assert.equal(result.results[0].ok, true)
  assert.equal(calls.length, 2)
  for (const call of calls) {
    const address = new URL(call.url)
    assert.equal(address.searchParams.get('_x_gantry'), 'true')
    assert.equal(address.searchParams.get('slack_route'), 'T1')
  }
})
