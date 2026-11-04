// Copyright © 2026 Jalapeno Labs
//
// slack_send.js against a fixture of Slack's web client: its `localConfig_v2` session and a fake
// web API in place of `fetch`. Run with `node --test api/src/connectors/*.test.mjs`.

// Core
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { test } from 'node:test'
import vm from 'node:vm'

const source = await readFile(new URL('./slack_send.js', import.meta.url), 'utf8')

const MOORESLAB = {
  id: 'T1',
  name: 'Mooreslab',
  url: 'https://mooreslab.slack.com/',
  token: 'xoxc-mooreslab',
  user_id: 'U0ME',
}

// Runs slack_send.js as the page would, with `request` in place of its argument. `answer` is
// the fake web API: given the address and form fields of a call, it answers Slack's JSON.
async function evaluate(request, { teams, answer = () => ({ ok: true }) }) {
  const calls = []
  const storage = {
    getItem: (key) => (key === 'localConfig_v2' && teams
      ? JSON.stringify({ teams })
      : null),
  }
  const fetch = async (url, init) => {
    const fields = Object.fromEntries(init.body.entries())
    calls.push({ url: String(url), fields, method: init.method, credentials: init.credentials })
    const body = await answer(String(url), fields)
    return { ok: true, status: 200, json: async () => body }
  }
  const script = source.replace('__ELYSIUM_REQUEST__', JSON.stringify(request))
  const answered = await vm.runInNewContext(script, {
    localStorage: storage,
    fetch,
    FormData,
    URL,
    AbortSignal,
  })
  const result = JSON.parse(JSON.stringify(answered))
  return { result, calls }
}

const REPLY = { team: 'T1', channel: 'C1', text: 'Friday &amp; noon', threadTs: '1730000000.000100' }

test('a reply is one chat.postMessage, on the workspace host, with its session', async () => {
  const { result, calls } = await evaluate(REPLY, {
    teams: { T1: MOORESLAB },
    answer: () => ({ ok: true, channel: 'C1', ts: '1730000100.000300', message: { text: 'echo' } }),
  })

  assert.deepEqual(calls, [{
    url: 'https://mooreslab.slack.com/api/chat.postMessage',
    method: 'POST',
    credentials: 'include',
    fields: {
      token: 'xoxc-mooreslab',
      channel: 'C1',
      text: 'Friday &amp; noon',
      thread_ts: '1730000000.000100',
    },
  }])
  assert.deepEqual(result, {
    signedIn: true,
    ok: true,
    error: null,
    channel: 'C1',
    ts: '1730000100.000300',
    teamUrl: 'https://mooreslab.slack.com/',
  })
})

test('a message outside a thread names no thread', async () => {
  const { calls } = await evaluate({ ...REPLY, threadTs: null }, { teams: { T1: MOORESLAB } })
  assert.equal(calls.length, 1)
  assert.equal('thread_ts' in calls[0].fields, false)
})

test('the session token never leaves the page', async () => {
  const { result } = await evaluate(REPLY, {
    teams: { T1: MOORESLAB },
    answer: () => ({ ok: true, channel: 'C1', ts: '1.2' }),
  })
  assert.ok(!JSON.stringify(result).includes('xoxc-'))
})

test('Slack refusing says why', async () => {
  const { result } = await evaluate(REPLY, {
    teams: { T1: MOORESLAB },
    answer: () => ({ ok: false, error: 'not_in_channel' }),
  })
  assert.equal(result.ok, false)
  assert.equal(result.error, 'not_in_channel')
})

test('a tab with no signed-in workspace posts nothing', async () => {
  const { result, calls } = await evaluate(REPLY, { teams: null })
  assert.equal(result.signedIn, false)
  assert.deepEqual(calls, [])
})

test('a workspace the tab is not signed in to posts nothing', async () => {
  const { result, calls } = await evaluate({ ...REPLY, team: 'T9' }, { teams: { T1: MOORESLAB } })
  assert.equal(result.error, 'team_not_signed_in')
  assert.deepEqual(calls, [])
})

test('a workspace stored at a host that is not Slack is never called', async () => {
  const elsewhere = { ...MOORESLAB, url: 'https://slack.com.example.org/' }
  const { result, calls } = await evaluate(REPLY, { teams: { T1: elsewhere } })
  assert.equal(result.signedIn, false)
  assert.deepEqual(calls, [])
})
