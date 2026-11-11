// Copyright © 2026 Jalapeno Labs
//
// teams_meetings.js against a fake of what a signed-in Teams holds: the reply chain store with a
// meeting's recording message and its call-ended message, shaped as a signed-in Teams stores
// them (ids and names replaced), the MSAL cache in localStorage, and a fake SharePoint in place
// of `fetch`. The page enforces Trusted Types as Teams does, so no parser takes a string. Run
// with `node --test api/src/connectors/*.test.mjs`.

// Core
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { test } from 'node:test'
import vm from 'node:vm'

const source = await readFile(new URL('./teams_meetings.js', import.meta.url), 'utf8')

const HOST = 'contoso-my.sharepoint.com'
const ITEM = `https://${HOST}/_api/v2.1/drives/b!drive-1/items/01ITEM`
const SHARE_LINK = `https://${HOST}/:v:/g/personal/ada_contoso_com/share-1`
const NOW_SECONDS = Math.floor(Date.now() / 1000)
const SHAREPOINT_TOKEN = {
  secret: 'eyJ.sharepoint-token',
  target: `https://${HOST}/Sites.FullControl.All https://${HOST}/.default`,
  expiresOn: String(NOW_SECONDS + 3600),
}
const GRAPH_TOKEN = {
  secret: 'eyJ.graph-token',
  target: 'https://graph.microsoft.com/.default',
  expiresOn: String(NOW_SECONDS + 3600),
}
const TOKENS = {
  'msal.2|home|login.windows.net|accesstoken|client|tenant|sharepoint|': SHAREPOINT_TOKEN,
  'msal.2|home|login.windows.net|accesstoken|client|tenant|graph|': GRAPH_TOKEN,
}

// The message Teams posts in a meeting's chat once its recording is ready, as stored.
function recordingMessage({ status = 'Success', host = HOST } = {}) {
  return {
    id: '1762795815790',
    messageType: 'RichText/Media_CallRecording',
    originalArrivalTime: 1762795815790,
    content: '<URIObject format_version="1.1" type="Video.2/CallRecording.1" uri="" version="1.0">'
      + `<RecordingStatus status="${status}" code="200" shortChunk="False"><amsErrorResult /></RecordingStatus>`
      + '<Title>Launch review &amp; demo</Title>'
      + '<MeetingOrganizerId value="8:orgid:ada-id" />'
      + '<RecordingStorage type="MeetingOrganizerOneDrive" userId="ada-id" />'
      + '<Identifiers><Id type="callId" value="call-1" /><Id type="chunkIndex" value="0" /></Identifiers>'
      + '<RecordingContent contentTypes="Recording+Transcript" timestamp="2026-11-10T17:30:15.7903791Z"'
      + ' duration="0:39:36.52" canVideoExpire="True">'
      + '<item type="video" uri="" />'
      + `<item type="onedriveForBusinessVideo" uri="https://${host}/:v:/g/personal/ada_contoso_com/share-1"`
      + ' driveId="b!drive-1" driveItemId="01ITEM" />'
      + `<item type="onedriveForBusinessTranscript" uri="https://${host}/personal/ada_contoso_com/_api/v2.1`
      + '/drives/b!drive-1/items/01ITEM/versions/current/media/transcripts/t-1/content" />'
      + '</RecordingContent></URIObject>',
  }
}

// The call's ended message, which names its organizer and who took part.
const CALL_ENDED = {
  id: '1762798192000',
  messageType: 'Event/Call',
  originalArrivalTime: 1762798192000,
  content: '<ended/><partlist alt="" count="2">'
    + '<part identity="8:orgid:ada-id"><name>8:orgid:ada-id</name><displayName>Ada Lovelace</displayName>'
    + '<duration>2376</duration></part>'
    + '<part identity="8:orgid:grace-id"><name>8:orgid:grace-id</name><displayName>Grace Hopper</displayName>'
    + '<duration>2300</duration></part></partlist>'
    + '<meetingDetails><meetingDetails><organizerUpn>ada@contoso.com</organizerUpn></meetingDetails></meetingDetails>'
    + '<callId>call-1</callId>',
}

class EnforcedDomParser {
  parseFromString() {
    throw new TypeError('Failed to execute \'parseFromString\': This document requires \'TrustedHTML\' assignment.')
  }
}

function fakeIndexedDb(messages) {
  const answer = (result) => {
    const request = {}
    queueMicrotask(() => {
      request.result = result
      request.onsuccess?.()
    })
    return request
  }
  const name = 'Teams:replychain-manager:react-web-client:tenant-id:ada-id:en-us'
  const chains = messages.map((message) => ({ replyChainId: message.id, messageMap: { [message.id]: message } }))
  return {
    databases: async () => [ { name }, { name: 'Teams:profiles:other' } ],
    open: () => answer({
      objectStoreNames: [ 'replychains' ],
      transaction: () => ({ objectStore: () => ({ getAll: () => answer(chains) }) }),
      close: () => {},
    }),
  }
}

// A SharePoint that holds one recording, as its API answers it.
function sharePoint(overrides = {}) {
  return (url) => {
    const path = new URL(url).pathname + new URL(url).search
    if (overrides[path]) {
      return overrides[path]
    }
    if (path.startsWith('/_api/v2.1/drives/b!drive-1/items/01ITEM?select=')) {
      return {
        body: {
          'id': '01ITEM',
          'file': { mimeType: 'video/mp4' },
          'video': { duration: 2376576 },
          '@content.downloadUrl': `https://${HOST}/personal/ada/_layouts/15/download.aspx?UniqueId=1&tempauth=signed`,
        },
      }
    }
    if (path === '/_api/v2.1/drives/b!drive-1/items/01ITEM/media/transcripts') {
      return {
        body: {
          value: [
            { id: 't-0', isDefault: false },
            { id: 't-1', isDefault: true },
          ],
        },
      }
    }
    if (path === '/_api/v2.1/drives/b!drive-1/items/01ITEM/media/transcripts/t-1/content?format=json') {
      return {
        body: {
          type: 'Transcript',
          entries: [
            {
              startOffset: '00:00:14.4571439',
              endOffset: '00:00:18.2171439',
              speakerDisplayName: 'Ada Lovelace',
              text: 'Shall we start?',
            },
            {
              startOffset: '00:00:19.6571439',
              endOffset: '00:00:22.5371439',
              speakerDisplayName: 'Grace Hopper',
              text: 'Yes.',
            },
          ],
        },
      }
    }
    return { status: 404, body: {} }
  }
}

// Runs teams_meetings.js as the page would: `messages` is the reply chain store, `stored` is
// localStorage, and `answer` the fake SharePoint.
async function evaluate(request, options = {}) {
  const {
    messages = [ recordingMessage(), CALL_ENDED ],
    stored = TOKENS,
    answer = sharePoint(),
    hostname = 'teams.cloud.microsoft',
  } = options
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
  const fetch = async (url, init) => {
    calls.push({ url, method: init?.method ?? 'GET', headers: init?.headers ?? {} })
    const { status = 200, body } = answer(url)
    return { ok: status < 300, status, json: async () => body }
  }
  const context = {
    indexedDB: fakeIndexedDb(messages),
    DOMParser: EnforcedDomParser,
    localStorage,
    fetch,
    location: { hostname },
    URL,
    Date,
  }
  const answered = await vm.runInNewContext(`(${source})(${JSON.stringify(request)})`, context)
  return { result: JSON.parse(JSON.stringify(answered)), calls }
}

test('a ready recording is listed by its drive item, with its meeting, read from markup alone', async () => {
  const { result, calls } = await evaluate({ action: 'list' })
  assert.equal(result.signedIn, true)
  assert.equal(result.found, true)
  assert.deepEqual(result.recordings, [ {
    item: ITEM,
    url: SHARE_LINK,
    title: 'Launch review & demo',
    startedAt: '2026-11-10T17:30:15.7903791Z',
    duration: '0:39:36.52',
    organizer: 'ada@contoso.com',
    participants: [ 'Ada Lovelace', 'Grace Hopper' ],
    hasTranscript: true,
  } ])
  assert.deepEqual(calls, [], 'listing reads the store and calls nothing')
})

test('a recording still being made, or kept anywhere but SharePoint, is not listed', async () => {
  const { result } = await evaluate({ action: 'list' }, {
    messages: [ recordingMessage({ status: 'ChunkFinished' }), recordingMessage({ host: 'example.org' }) ],
  })
  assert.deepEqual(result.recordings, [])
})

test('a recording without its call-ended message is listed without who took part', async () => {
  const { result } = await evaluate({ action: 'list' }, { messages: [ recordingMessage() ] })
  assert.equal(result.recordings[0].organizer, '')
  assert.deepEqual(result.recordings[0].participants, [])
})

test('a page without the store says so', async () => {
  const context = {
    indexedDB: { databases: async () => [ { name: 'Teams:profiles:other' } ] },
    location: { hostname: 'teams.cloud.microsoft' },
  }
  const answered = await vm.runInNewContext(`(${source})({ "action": "list" })`, context)
  assert.equal(answered.found, false)
})

test('fetching reads the drive item, its default transcript as JSON, and its download, by GET alone', async () => {
  const { result, calls } = await evaluate({ action: 'fetch', item: ITEM })
  assert.deepEqual(result, {
    signedIn: true,
    ok: true,
    recording: {
      url: `https://${HOST}/personal/ada/_layouts/15/download.aspx?UniqueId=1&tempauth=signed`,
      mimeType: 'video/mp4',
      isVideo: true,
    },
    transcript: [
      { start: '00:00:14.4571439', end: '00:00:18.2171439', speaker: 'Ada Lovelace', text: 'Shall we start?' },
      { start: '00:00:19.6571439', end: '00:00:22.5371439', speaker: 'Grace Hopper', text: 'Yes.' },
    ],
  })
  assert.deepEqual(calls.map((call) => call.method), [ 'GET', 'GET', 'GET' ])
  assert.ok(calls.every((call) => new URL(call.url).host === HOST))
  assert.ok(calls.every((call) => call.headers.Authorization === 'Bearer eyJ.sharepoint-token'))
})

test('the token never leaves the page', async () => {
  const { result } = await evaluate({ action: 'fetch', item: ITEM })
  assert.ok(!JSON.stringify(result).includes('eyJ'))
})

test('an address that is not a SharePoint drive item is never called', async () => {
  for (const item of [
    'https://evil.example.org/_api/v2.1/drives/b!drive-1/items/01ITEM',
    `https://${HOST}/_api/v2.1/drives/b!drive-1/items/01ITEM/children`,
    `http://${HOST}/_api/v2.1/drives/b!drive-1/items/01ITEM`,
    'not a url',
  ]) {
    const { result, calls } = await evaluate({ action: 'fetch', item })
    assert.equal(result.error, 'not_a_drive_item', item)
    assert.deepEqual(calls, [])
  }
})

test('an expired token is not used, and no token at all says so', async () => {
  const expired = { ...SHAREPOINT_TOKEN, expiresOn: String(NOW_SECONDS - 10) }
  const stale = await evaluate({ action: 'fetch', item: ITEM }, {
    stored: { 'msal.2|accesstoken|sharepoint': expired },
  })
  assert.equal(stale.result.error, 'token_expired')
  assert.deepEqual(stale.calls, [])
  const none = await evaluate({ action: 'fetch', item: ITEM }, { stored: { 'msal.2|accesstoken|graph': GRAPH_TOKEN } })
  assert.equal(none.result.error, 'token_not_found')
})

test('a refusal names its status and step, and 401 reads as signed out', async () => {
  const missing = await evaluate({ action: 'fetch', item: ITEM }, { answer: () => ({ status: 404, body: {} }) })
  assert.deepEqual(missing.result, { signedIn: true, ok: false, error: 'http_error', status: 404, step: 'item' })
  const unauthorized = await evaluate({ action: 'fetch', item: ITEM }, { answer: () => ({ status: 401, body: {} }) })
  assert.equal(unauthorized.result.signedIn, false)
})

test('a recording whose download is blocked is fetched for its transcript alone', async () => {
  const blocked = sharePoint({
    '/_api/v2.1/drives/b!drive-1/items/01ITEM?select=id,file,video,audio,@content.downloadUrl': {
      body: { id: '01ITEM', file: { mimeType: 'video/mp4' }, video: {} },
    },
  })
  const { result } = await evaluate({ action: 'fetch', item: ITEM }, { answer: blocked })
  assert.equal(result.ok, true)
  assert.equal(result.recording, null)
  assert.equal(result.transcript.length, 2)
})

test('a tab on a sign-in page reads and calls nothing', async () => {
  const { result, calls } = await evaluate({ action: 'fetch', item: ITEM }, { hostname: 'login.microsoftonline.com' })
  assert.equal(result.signedIn, false)
  assert.deepEqual(calls, [])
})
