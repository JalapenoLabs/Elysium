// Copyright © 2026 Jalapeno Labs
//
// hive.js against a fake Fireflies web app: its `localStorage` session and a fake `/api/v4/hive`
// in place of `fetch`. Run with `node --test api/src/meetings/*/*.test.mjs`.

// Core
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { test } from 'node:test'
import vm from 'node:vm'

const source = await readFile(new URL('./hive.js', import.meta.url), 'utf8')

const SESSION = 'fireflies-session-redacted'
const QUERY = 'query ElysiumRecording($meetingNoteId: String) { meetingNote(_id: $meetingNoteId) { video_url } }'

// Runs hive.js as the page would, with `request` in place of its argument. `answer` is the fake
// endpoint: given the request's body, it answers `{ status, body }`.
async function evaluate(request, { token = SESSION, answer = () => ({ status: 200, body: { data: {} } }) } = {}) {
  const calls = []
  const localStorage = {
    getItem: (key) => (key === 'AUTHORIZATION'
      ? token
      : null),
  }
  const fetch = async (url, init) => {
    calls.push({ url: String(url), init })
    const { status, body } = await answer(JSON.parse(init.body))
    return { status, json: async () => body }
  }
  const script = source.replace('__ELYSIUM_REQUEST__', JSON.stringify(request))
  const answered = await vm.runInNewContext(script, { localStorage, fetch })
  // DevTools hands the API the value as JSON (`returnByValue`), so assertions compare plain values.
  return { result: JSON.parse(JSON.stringify(answered)), calls }
}

test('a page without a session answers signed out and asks nothing', async () => {
  const { result, calls } = await evaluate({ calls: [ { query: QUERY, variables: {} } ] }, { token: null })
  assert.deepEqual(result, { signedIn: false, results: [] })
  assert.deepEqual(calls, [])
})

test('each query is asked of the endpoint with the session, which never comes back', async () => {
  const { result, calls } = await evaluate(
    { calls: [ { query: QUERY, variables: { meetingNoteId: '01A' } } ] },
    { answer: (body) => ({ status: 200, body: { data: { asked: body.variables.meetingNoteId } } }) },
  )

  assert.equal(result.signedIn, true)
  assert.deepEqual(result.results, [ { status: 200, data: { asked: '01A' }, errors: [] } ])
  assert.equal(calls.length, 1)
  assert.equal(calls[0].url, '/api/v4/hive')
  assert.equal(calls[0].init.method, 'POST')
  assert.equal(calls[0].init.credentials, 'include')
  assert.equal(calls[0].init.headers.authorization, SESSION)
  assert.ok(!JSON.stringify(result).includes(SESSION), 'the session stays in the page')
})

test('a mutation is refused before anything is sent', async () => {
  const mutation = 'mutation DeleteMeeting($id: String!) { deleteMeeting(id: $id) }'
  const disguised = 'query Read { meetingNote(_id: "x") { title } } mutation Delete { deleteMeeting(id: "x") }'
  const { result, calls } = await evaluate({ calls: [
    { query: mutation, variables: {} },
    { query: disguised, variables: {} },
  ] })

  assert.deepEqual(calls, [])
  assert.deepEqual(result.results.map((entry) => entry.errors[0].code), [ 'not_a_query', 'not_a_query' ])
})

test('Fireflies errors come back as their messages and codes', async () => {
  const { result } = await evaluate({ calls: [ { query: QUERY, variables: {} } ] }, {
    answer: () => ({
      status: 200,
      body: { data: null, errors: [ { message: 'gone', extensions: { code: 'NOT_FOUND_ERROR' } } ] },
    }),
  })
  assert.deepEqual(result.results, [
    { status: 200, data: null, errors: [ { message: 'gone', code: 'NOT_FOUND_ERROR' } ] },
  ])
})

test('an answer that is not JSON keeps its status', async () => {
  const { result } = await evaluate({ calls: [ { query: QUERY, variables: {} } ] }, {
    answer: () => ({ status: 502, body: undefined }),
  })
  assert.deepEqual(result.results, [ { status: 502, data: null, errors: [] } ])
})
