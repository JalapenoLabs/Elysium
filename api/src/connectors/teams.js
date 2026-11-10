// Copyright © 2026 Jalapeno Labs
//
// Reads what Microsoft Teams on the web has already stored about its chats and channels, in
// its own signed-in tab, over DevTools. The API evaluates this as one expression, a function
// applied to `{ since, maxMessages }`, and reads back:
//
//   { signedIn, found, me, conversations: [...], messages: [...], myThreadIds: [...], databases }
//
// New Teams keeps its conversations and their messages in IndexedDB, in databases named
// `Teams:conversation-manager:...:<tenant id>:<user id>:<locale>` and
// `Teams:replychain-manager:...`. Those records carry each message's id, sender, time, and
// mentions, and each conversation's read horizon, so reading them answers what is new and what
// is unread without touching the page. Nothing here clicks, opens a chat, or writes, so reading
// leaves unread messages unread. The API decides what wants attention (docs/connectors.md);
// this only reads.
//
// Field names are read defensively: the store is the web client's own, not a published API.
// When a database is missing, `found` is false and `databases` lists what the page does hold,
// so a check can say what it saw.
((options) => {
  const TEXT_MAX_CHARACTERS = 4000

  // Every value in a store, read in one read-only transaction.
  function readAll(database, storeName) {
    return new Promise((resolve, reject) => {
      const request = database
        .transaction(storeName, 'readonly')
        .objectStore(storeName)
        .getAll()
      request.onsuccess = () => resolve(request.result || [])
      request.onerror = () => reject(request.error)
    })
  }

  function open(name) {
    return new Promise((resolve, reject) => {
      const request = indexedDB.open(name)
      request.onsuccess = () => resolve(request.result)
      request.onerror = () => reject(request.error)
      // A database the page has not created yet would be created empty by opening it.
      request.onupgradeneeded = () => {
        request.transaction.abort()
        reject(new Error(`no database ${name}`))
      }
    })
  }

  // Every value in every store of the first database whose name holds `marker`.
  async function readDatabase(names, marker) {
    const name = names.find((candidate) => candidate.includes(marker))
    if (!name) {
      return { name: null, values: [] }
    }
    const database = await open(name)
    try {
      const values = []
      for (const storeName of [ ...database.objectStoreNames ]) {
        values.push(...await readAll(database, storeName))
      }
      return { name, values }
    }
    finally {
      database.close()
    }
  }

  // An arrival time as milliseconds, whether stored as a number or an ISO string.
  function milliseconds(value) {
    if (typeof value === 'number') {
      return value
    }
    if (typeof value === 'string') {
      const parsed = Date.parse(value)
      return Number.isNaN(parsed)
        ? null
        : parsed
    }
    return null
  }

  // An MRI such as `8:orgid:<id>`, from a bare MRI or a contacts URL ending in one.
  function mri(value) {
    if (typeof value !== 'string') {
      return null
    }
    const match = value.match(/(\d+:[^/]+)$/)
    return match
      ? match[1]
      : value
  }

  // A tag, read past quoted attribute values, which may hold a `>` of their own.
  const TAG = /<\/?([a-zA-Z][a-zA-Z0-9:-]*)(?:[^>"']|"[^"]*"|'[^']*')*>/g
  // Tags that end a line of text, so the words on either side stay apart. Every other tag
  // (bold, a link, a mention) sits inside a word's line and leaves nothing behind.
  const BREAKING_TAGS = new Set([
    'blockquote', 'br', 'div', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'hr', 'li', 'ol', 'p',
    'pre', 'table', 'td', 'th', 'tr', 'ul',
  ])
  // The named references HTML's serializer writes, which is how Teams stores what its editor
  // composed, and `&apos;`; any other named reference is left as written.
  const NAMED_REFERENCES = { amp: '&', lt: '<', gt: '>', quot: '"', apos: '\'', nbsp: '\u00a0' }

  function decodedReference(reference, name) {
    if (name[0] !== '#') {
      return NAMED_REFERENCES[name] ?? reference
    }
    const codePoint = name[1] === 'x' || name[1] === 'X'
      ? Number.parseInt(name.slice(2), 16)
      : Number.parseInt(name.slice(1), 10)
    const isCharacter = Number.isInteger(codePoint) && codePoint > 0 && codePoint <= 0x10FFFF
    return isCharacter
      ? String.fromCodePoint(codePoint)
      : reference
  }

  // A message's HTML as the text it reads as. Teams enforces Trusted Types
  // (`require-trusted-types-for 'script'`) and its CSP names every policy it allows, none of
  // them ours, so markup cannot be handed to a parser (`DOMParser`, `innerHTML`, a template).
  // The text is read off the markup instead: tags dropped, references decoded. Nothing is
  // rendered, so this is safe on what anyone in the chat wrote.
  function plainText(html) {
    if (typeof html !== 'string' || !html) {
      return ''
    }
    const text = html
      .replace(TAG, (tag, name) => (BREAKING_TAGS.has(name.toLowerCase())
        ? ' '
        : ''))
      .replace(/&(#[xX][0-9a-fA-F]+|#[0-9]+|[a-zA-Z]+);/g, decodedReference)
    return text.replace(/\s+/g, ' ').trim().slice(0, TEXT_MAX_CHARACTERS)
  }

  function message(raw, conversationId, parentId) {
    return {
      id: String(raw.id ?? raw.clientMessageId ?? raw.clientmessageid ?? ''),
      conversationId: String(raw.conversationId ?? raw.conversationid ?? conversationId ?? ''),
      parentId: parentId ?? raw.parentMessageId ?? raw.replyChainId ?? null,
      createdAtMs: milliseconds(
        raw.originalArrivalTime ?? raw.originalarrivaltime ?? raw.composeTime ?? raw.composetime,
      ),
      from: mri(raw.creator ?? raw.from),
      fromName: raw.imDisplayName ?? raw.imdisplayname ?? '',
      messageType: raw.messageType ?? raw.messagetype ?? '',
      text: plainText(raw.content),
      mentions: raw.properties?.mentions ?? raw.mentions ?? null,
    }
  }

  return (async () => {
    const onSignIn = /login\.(microsoftonline|live)\.com/.test(location.hostname)
    if (onSignIn) {
      return { signedIn: false, found: false, me: null, conversations: [], messages: [], myThreadIds: [], databases: [] }
    }

    const names = (await indexedDB.databases())
      .map((database) => database.name)
      .filter(Boolean)
    const conversationStore = await readDatabase(names, 'conversation-manager')
    const replyChainStore = await readDatabase(names, 'replychain-manager')

    // The database name ends in `<tenant id>:<user id>:<locale>`; the user's MRI is built from
    // their id, which their own messages carry as `8:orgid:<user id>`.
    const owner = conversationStore.name?.split(':').slice(-2)[0]
    const me = owner
      ? `8:orgid:${owner}`
      : null

    const conversations = conversationStore.values
      .filter((raw) => typeof raw?.id === 'string')
      .map((raw) => ({
        id: raw.id,
        topic: raw.threadProperties?.topic ?? raw.threadProperties?.spaceThreadTopic ?? raw.title ?? '',
        threadType: raw.threadProperties?.threadType ?? raw.type ?? '',
        consumptionHorizon: raw.properties?.consumptionhorizon ?? raw.properties?.consumptionHorizon ?? null,
        lastMessageAtMs: milliseconds(
          raw.lastMessage?.originalarrivaltime ?? raw.lastMessage?.composetime ?? raw.lastMessageTime,
        ),
      }))

    // A reply chain is one thread: its root and replies, in `messageMap` or `messages`.
    const allMessages = []
    for (const chain of replyChainStore.values) {
      const map = chain?.messageMap ?? chain?.messages ?? {}
      const parentId = chain?.replyChainId ?? null
      for (const raw of Object.values(map)) {
        if (raw && typeof raw === 'object') {
          allMessages.push(message(raw, chain?.conversationId, parentId))
        }
      }
    }

    // Threads the user wrote in, read across everything stored, not only what is new.
    const myThreadIds = [ ...new Set(allMessages
      .filter((entry) => me && entry.from === me && entry.parentId)
      .map((entry) => `${entry.conversationId}/${entry.parentId}`)) ]

    const since = options.since ?? 0
    const messages = allMessages
      .filter((entry) => entry.id && entry.createdAtMs !== null && entry.createdAtMs > since)
      .sort((left, right) => right.createdAtMs - left.createdAtMs)
      .slice(0, options.maxMessages)

    return {
      signedIn: true,
      found: Boolean(conversationStore.name),
      me,
      conversations,
      messages,
      myThreadIds,
      databases: conversationStore.name
        ? []
        : names,
    }
  })()
})
