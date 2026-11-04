// Copyright © 2026 Jalapeno Labs
//
// teams_send.js against a fixture of Teams on the web: the MSAL cache it keeps in localStorage,
// the chat service calls its page has made, and a fake chat service in place of `fetch`. The
// shapes follow what the web client is known to keep and send; none of it has yet been checked
// against a signed-in Teams. Run with `node --test api/src/connectors/*.test.mjs`.

// Core
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { test } from 'node:test'
import vm from 'node:vm'

const source = await readFile(new URL('./teams_send.js', import.meta.url), 'utf8')

const NOW_SECONDS = Math.floor(Date.now() / 1000)
const CHAT_TOKEN_KEY = 'home-login.windows.net-accesstoken-5e3ce6c0-tenant-https://ic3.teams.office.com/.default'
const CHAT_TOKEN = {
  secret: 'eyJ.chat-token',
  target: 'https://ic3.teams.office.com/.default',
  expiresOn: String(NOW_SECONDS + 3600),
}
const OTHER_TOKEN_KEY = 'home-login.windows.net-accesstoken-5e3ce6c0-tenant-https://graph.microsoft.com/.default'
const OTHER_TOKEN = { secret: 'eyJ.graph-token', target: 'https://graph.microsoft.com/.default', expiresOn: CHAT_TOKEN.expiresOn }
const CHAT_SERVICE_TIMING = 'https://teams.cloud.microsoft/api/chatsvc/amer/v1/users/ME/conversations?view=mychats'

// Runs teams_send.js as the page would. `stored` is localStorage, `timings` the resource URLs the
// page loaded, and `answer` the fake chat service: given the address and request, its response.
async function evaluate(request, { stored = {}, timings = [ CHAT_SERVICE_TIMING ], hostname = 'teams.cloud.microsoft', answer }) {
  const calls = []
  const keys = Object.keys(stored)
  const localStorage = {
    get length() {
      return keys.length
    },
    key: (index) => keys[index] ?? null,
    getItem: (key) => (key in stored
      ? JSON.stringify(stored[key])
      : null),
  }
  const performance = {
    getEntriesByType: (type) => (type === 'resource'
      ? timings.map((name) => ({ name }))
      : []),
  }
  const fetch = async (url, init) => {
    calls.push({ url, method: init.method, headers: init.headers, body: JSON.parse(init.body) })
    const { status = 201, body = { OriginalArrivalTime: 1730000400000 } } = answer?.(url) ?? {}
    return { ok: status < 300, status, json: async () => body }
  }
  const script = source.replace('__ELYSIUM_REQUEST__', JSON.stringify(request))
  const answered = await vm.runInNewContext(script, {
    localStorage,
    performance,
    fetch,
    location: { hostname },
    AbortSignal,
    JSON,
    Date,
    Number,
    String,
    Set,
  })
  return { result: JSON.parse(JSON.stringify(answered)), calls }
}

const CHANNEL_REPLY = {
  conversationId: '19:launch@thread.tacv2',
  rootMessageId: '1730000100000',
  text: 'Friday <noon> & "after"\nworks',
  senderName: 'Alex Navarro',
}

test('a channel reply posts under the thread, to the chat service the page uses, with its token', async () => {
  const { result, calls } = await evaluate(CHANNEL_REPLY, {
    stored: { [OTHER_TOKEN_KEY]: OTHER_TOKEN, [CHAT_TOKEN_KEY]: CHAT_TOKEN },
  })

  assert.equal(calls.length, 1)
  const [ call ] = calls
  assert.equal(
    call.url,
    'https://teams.cloud.microsoft/api/chatsvc/amer/v1/users/ME/conversations/'
      + encodeURIComponent('19:launch@thread.tacv2;messageid=1730000100000')
      + '/messages',
  )
  assert.equal(call.method, 'POST')
  assert.equal(call.headers.Authorization, 'Bearer eyJ.chat-token')
  assert.equal(call.body.content, '<p>Friday &lt;noon&gt; &amp; &quot;after&quot;<br>works</p>')
  assert.equal(call.body.messagetype, 'RichText/Html')
  assert.equal(call.body.contenttype, 'text')
  assert.equal(call.body.imdisplayname, 'Alex Navarro')
  assert.match(call.body.clientmessageid, /^\d+$/)
  assert.deepEqual(result, { signedIn: true, ok: true, arrivalTime: 1730000400000 })
})

test('a message to a chat posts to the chat itself', async () => {
  const { calls } = await evaluate({ ...CHANNEL_REPLY, conversationId: '19:ada_me@unq.gbl.spaces', rootMessageId: null }, {
    stored: { [CHAT_TOKEN_KEY]: CHAT_TOKEN },
  })
  assert.ok(calls[0].url.endsWith(`/conversations/${encodeURIComponent('19:ada_me@unq.gbl.spaces')}/messages`))
})

test('the token never leaves the page', async () => {
  const { result } = await evaluate(CHANNEL_REPLY, { stored: { [CHAT_TOKEN_KEY]: CHAT_TOKEN } })
  assert.ok(!JSON.stringify(result).includes('eyJ'))
})

test('an expired token is not used, and says so', async () => {
  const expired = { ...CHAT_TOKEN, expiresOn: String(NOW_SECONDS - 10) }
  const { result, calls } = await evaluate(CHANNEL_REPLY, { stored: { [CHAT_TOKEN_KEY]: expired } })
  assert.deepEqual(result, { signedIn: true, ok: false, error: 'token_expired' })
  assert.deepEqual(calls, [])
})

test('no chat token, as an encrypted cache looks, says so', async () => {
  const { result, calls } = await evaluate(CHANNEL_REPLY, { stored: { [OTHER_TOKEN_KEY]: OTHER_TOKEN } })
  assert.equal(result.error, 'token_not_found')
  assert.deepEqual(calls, [])
})

test('a chat service the page has not called, or not on a Teams host, is not guessed', async () => {
  for (const timings of [ [], [ 'https://evil.example.org/api/chatsvc/amer/v1/users/ME/conversations' ] ]) {
    const { result, calls } = await evaluate(CHANNEL_REPLY, { stored: { [CHAT_TOKEN_KEY]: CHAT_TOKEN }, timings })
    assert.equal(result.error, 'region_unknown')
    assert.deepEqual(calls, [])
  }
})

test('a refusal names its status, and 401 reads as signed out', async () => {
  const forbidden = await evaluate(CHANNEL_REPLY, { stored: { [CHAT_TOKEN_KEY]: CHAT_TOKEN }, answer: () => ({ status: 403 }) })
  assert.deepEqual(forbidden.result, { signedIn: true, ok: false, error: 'http_error', status: 403 })
  const unauthorized = await evaluate(CHANNEL_REPLY, { stored: { [CHAT_TOKEN_KEY]: CHAT_TOKEN }, answer: () => ({ status: 401 }) })
  assert.equal(unauthorized.result.signedIn, false)
})

test('a tab on a sign-in page posts nothing', async () => {
  const { result, calls } = await evaluate(CHANNEL_REPLY, {
    stored: { [CHAT_TOKEN_KEY]: CHAT_TOKEN },
    hostname: 'login.microsoftonline.com',
  })
  assert.equal(result.signedIn, false)
  assert.deepEqual(calls, [])
})
