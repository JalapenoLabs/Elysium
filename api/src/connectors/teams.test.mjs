// Copyright © 2026 Jalapeno Labs
//
// teams.js against a fake of the IndexedDB store Teams keeps, on a page that enforces Trusted
// Types as Teams does (`require-trusted-types-for 'script'`, with a `trusted-types` list that
// names none of Elysium's policies): every HTML parser refuses a plain string. The message
// markup is shaped like what a signed-in Teams stores. Run with
// `node --test api/src/connectors/*.test.mjs`.

// Core
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { test } from 'node:test'
import vm from 'node:vm'

const source = await readFile(new URL('./teams.js', import.meta.url), 'utf8')

const TENANT_ID = '5e3ce6c0-0000-4000-8000-000000000001'
const MY_ID = 'aaaa1111-0000-4000-8000-000000000002'
// How Teams names its databases: the tenant, then the signed-in person, then the locale.
const SUFFIX = `react-web-client:${TENANT_ID}:${MY_ID}:en-us`

// The error Chromium throws when a page under Trusted Types hands a parser a string.
function trustedTypesRefusal(sink) {
  return new TypeError(`Failed to execute '${sink}': This document requires 'TrustedHTML' assignment.`)
}

class EnforcedDomParser {
  parseFromString() {
    throw trustedTypesRefusal('parseFromString')
  }
}

// An IndexedDB holding `databases`, `{ name: { storeName: [values] } }`, read the way the
// browser answers: requests whose handlers run after they are set.
function fakeIndexedDb(databases) {
  const answer = (request, result) => {
    queueMicrotask(() => {
      request.result = result
      request.onsuccess?.()
    })
    return request
  }
  return {
    databases: async () => Object.keys(databases).map((name) => ({ name })),
    open: (name) => {
      const stores = databases[name]
      const database = {
        objectStoreNames: Object.keys(stores),
        transaction: (storeName) => ({
          objectStore: () => ({ getAll: () => answer({}, stores[storeName]) }),
        }),
        close: () => {},
      }
      return answer({}, database)
    },
  }
}

async function evaluate(messages, { hostname = 'teams.cloud.microsoft' } = {}) {
  const indexedDB = fakeIndexedDb({
    [`Teams:conversation-manager:${SUFFIX}`]: {
      conversations: [ { id: '19:chat@unq.gbl.spaces', threadProperties: {}, properties: {} } ],
    },
    [`Teams:replychain-manager:${SUFFIX}`]: {
      replychains: [ { replyChainId: '1', conversationId: '19:chat@unq.gbl.spaces', messageMap: messages } ],
    },
  })
  const context = {
    indexedDB,
    DOMParser: EnforcedDomParser,
    location: { hostname },
    Date,
  }
  const answered = await vm.runInNewContext(`(${source})({ "since": 0, "maxMessages": 200 })`, context)
  return JSON.parse(JSON.stringify(answered))
}

function stored(id, content, messageType = 'RichText/Html', creator = '8:orgid:bbbb-2222') {
  return {
    id,
    originalArrivalTime: new Date(1730000000000 + Number(id)).toISOString(),
    creator,
    imDisplayName: 'Ada Lovelace',
    messageType,
    content,
  }
}

function textOf(answer, id) {
  return answer.messages.find((message) => message.id === id).text
}

test('a page that enforces Trusted Types is read, without handing markup to a parser', async () => {
  const answer = await evaluate({
    a: stored('1', '<p>Are we launching <b>Friday</b>?</p>'),
  })
  assert.equal(answer.signedIn, true)
  assert.equal(answer.found, true)
  assert.equal(textOf(answer, '1'), 'Are we launching Friday?')
})

test('words on either side of a line break, a paragraph, or a list item stay apart', async () => {
  const answer = await evaluate({
    a: stored('1', '<p>First</p>\r\n<p> </p>\r\n<ul>\r\n<li>one</li><li>two</li></ul>'),
    b: stored('2', '<pre class="language-text"><code>Name: Ada<br />Role: lead</code></pre>'),
  })
  assert.equal(textOf(answer, '1'), 'First one two')
  assert.equal(textOf(answer, '2'), 'Name: Ada Role: lead')
})

test('inline markup, a mention, and a reply quote read as their words', async () => {
  const answer = await evaluate({
    a: stored(
      '1',
      '<blockquote itemscope itemtype="http://schema.skype.com/Reply" itemid="17">'
        + '<strong itemprop="mri" itemid="8:orgid:cccc">Grace Hopper</strong><span itemprop="time" itemid="17"></span>'
        + '<p itemprop="preview">Ship it?</p></blockquote>\r\n'
        + '<p><span itemtype="http://schema.skype.com/Mention" itemscope itemid="0">Alex</span> yes, '
        + '<a href="https://example.org/?a=1&amp;b=2" title="x > y">the doc</a></p>',
    ),
  })
  assert.equal(textOf(answer, '1'), 'Grace Hopper Ship it? Alex yes, the doc')
})

test('character references are decoded after the tags are gone, so escaped markup stays text', async () => {
  const answer = await evaluate({
    a: stored('1', '<p>Notes &amp; plans: use &lt;div&gt; &quot;here&quot; &#39;now&#39; &#x1F680; &copy;</p>'),
  })
  assert.equal(textOf(answer, '1'), 'Notes & plans: use <div> "here" \'now\' 🚀 &copy;')
})

test('a plain text message keeps what it says, even a comparison that looks like a tag', async () => {
  const answer = await evaluate({
    a: stored('1', 'If 5 < 6 and 7 > 3, ship', 'Text'),
  })
  assert.equal(textOf(answer, '1'), 'If 5 < 6 and 7 > 3, ship')
})

test('the person is the user the database is named for, not its tenant', async () => {
  const answer = await evaluate({
    a: stored('1', '<p>Mine</p>', 'RichText/Html', `https://notifications.skype.net/v1/users/ME/contacts/8:orgid:${MY_ID}`),
  })
  assert.equal(answer.me, `8:orgid:${MY_ID}`)
  assert.equal(answer.messages[0].from, answer.me)
})

test('a tab on a sign-in page reads nothing', async () => {
  const answer = await evaluate({}, { hostname: 'login.microsoftonline.com' })
  assert.equal(answer.signedIn, false)
  assert.deepEqual(answer.messages, [])
})
