// Copyright © 2026 Jalapeno Labs
//
// Reads the person's recorded Teams meetings in Teams' own signed-in tab, over DevTools. The API
// evaluates this as one expression, a function applied to its request, and reads back one of:
//
//   { action: 'list' }                → { signedIn, found, recordings: [...] }
//   { action: 'fetch', item: <url> }  → { signedIn, ok, recording, transcript, error?, status? }
//
// Listing reads the store Teams keeps in IndexedDB (`Teams:replychain-manager:...`), which holds
// the message Teams posts in a meeting's chat once its recording is ready
// (`RichText/Media_CallRecording`, status `Success`): the recording's drive item in the
// organizer's OneDrive, its title, start, and length, and the call it belongs to, whose
// call-ended message (`Event/Call`) names the organizer and who took part. Nothing is opened,
// so reading leaves the chat as it was.
//
// Fetching reads one recording's drive item from SharePoint's own API, as the web client does,
// with the access token Teams keeps for that SharePoint host in its MSAL cache (localStorage):
// the item's pre-authenticated download address (`@content.downloadUrl`), and its transcript,
// as `?format=json`, whose entries name their speaker; the WebVTT Teams serves names none.
// Every call is a GET. Nothing Microsoft's AI wrote (the recap, notes, or summaries) is asked
// for. The token never leaves this page; the download address does, since the API downloads the
// recording itself, and it expires within the hour.
//
// The store and the message are the web client's own, not a published API, so fields are read
// defensively, and the message's markup is read with patterns rather than a parser, which
// Teams' Trusted Types policy refuses (see teams.js).
((request) => {
  const SHAREPOINT_HOST = /^[a-z0-9-]+\.sharepoint\.com$/
  // A token this close to expiring could lapse on the way.
  const EXPIRY_MARGIN_SECONDS = 60

  function readAll(database, storeName) {
    return new Promise((resolve, reject) => {
      const read = database
        .transaction(storeName, 'readonly')
        .objectStore(storeName)
        .getAll()
      read.onsuccess = () => resolve(read.result || [])
      read.onerror = () => reject(read.error)
    })
  }

  function open(name) {
    return new Promise((resolve, reject) => {
      const opening = indexedDB.open(name)
      opening.onsuccess = () => resolve(opening.result)
      opening.onerror = () => reject(opening.error)
      // A database the page has not created yet would be created empty by opening it.
      opening.onupgradeneeded = () => {
        opening.transaction.abort()
        reject(new Error(`no database ${name}`))
      }
    })
  }

  // Every message in the reply chain store, or null when the page holds no such store.
  async function storedMessages() {
    const names = (await indexedDB.databases())
      .map((database) => database.name)
      .filter(Boolean)
    const name = names.find((candidate) => candidate.includes('replychain-manager'))
    if (!name) {
      return null
    }
    const database = await open(name)
    try {
      const messages = []
      for (const storeName of [ ...database.objectStoreNames ]) {
        for (const chain of await readAll(database, storeName)) {
          const map = chain?.messageMap ?? chain?.messages ?? {}
          for (const raw of Object.values(map)) {
            if (raw && typeof raw === 'object' && typeof raw.content === 'string') {
              messages.push(raw)
            }
          }
        }
      }
      return messages
    }
    finally {
      database.close()
    }
  }

  // An attribute value or element text as written in the message's markup.
  function decoded(text) {
    return text.replace(/&(#[xX][0-9a-fA-F]+|#[0-9]+|amp|lt|gt|quot|apos);/g, (reference, name) => {
      const named = { amp: '&', lt: '<', gt: '>', quot: '"', apos: '\'' }[name]
      if (named) {
        return named
      }
      const codePoint = name[1] === 'x' || name[1] === 'X'
        ? Number.parseInt(name.slice(2), 16)
        : Number.parseInt(name.slice(1), 10)
      if (!Number.isInteger(codePoint) || codePoint <= 0 || codePoint > 0x10FFFF) {
        return reference
      }
      return String.fromCodePoint(codePoint)
    })
  }

  // The attributes of the first `<tag ...>` in `markup` whose attributes match `filter`.
  function attributesOf(markup, tag, filter = () => true) {
    for (const match of markup.matchAll(new RegExp(`<${tag}\\b((?:[^>"']|"[^"]*"|'[^']*')*)>`, 'g'))) {
      const attributes = {}
      for (const attribute of match[1].matchAll(/([a-zA-Z_:][\w:.-]*)="([^"]*)"/g)) {
        attributes[attribute[1]] = decoded(attribute[2])
      }
      if (filter(attributes)) {
        return attributes
      }
    }
    return null
  }

  function textOf(markup, tag) {
    const match = new RegExp(`<${tag}>([^<]*)</${tag}>`).exec(markup)
    if (!match) {
      return null
    }
    return decoded(match[1]).trim()
  }

  // Who took part in each call, and who organized it, from the calls' ended messages.
  function callDetails(messages) {
    const byCall = new Map()
    for (const raw of messages) {
      const messageType = raw.messageType ?? raw.messagetype ?? ''
      const callId = textOf(raw.content, 'callId')
      if (messageType !== 'Event/Call' || !callId || !raw.content.includes('<partlist')) {
        continue
      }
      const participants = []
      for (const part of raw.content.matchAll(/<part identity="([^"]*)">([\s\S]*?)<\/part>/g)) {
        participants.push({ id: decoded(part[1]), name: textOf(part[2], 'displayName') ?? '' })
      }
      byCall.set(callId, { participants, organizerUpn: textOf(raw.content, 'organizerUpn') })
    }
    return byCall
  }

  async function list() {
    const messages = await storedMessages()
    if (!messages) {
      return { signedIn: true, found: false, recordings: [] }
    }
    const calls = callDetails(messages)
    const recordings = []
    for (const raw of messages) {
      const messageType = raw.messageType ?? raw.messagetype ?? ''
      const status = attributesOf(raw.content, 'RecordingStatus')?.status
      if (messageType !== 'RichText/Media_CallRecording' || status !== 'Success') {
        continue
      }
      const video = attributesOf(raw.content, 'item', (item) => item.type === 'onedriveForBusinessVideo')
      const host = video?.uri && URL.canParse(video.uri)
        ? new URL(video.uri).host
        : null
      if (!host || !SHAREPOINT_HOST.test(host) || !video.driveId || !video.driveItemId) {
        continue
      }
      const content = attributesOf(raw.content, 'RecordingContent') ?? {}
      const callId = attributesOf(raw.content, 'Id', (id) => id.type === 'callId')?.value ?? null
      const call = calls.get(callId)
      const organizerId = attributesOf(raw.content, 'MeetingOrganizerId')?.value ?? null
      const organizerName = call?.participants.find((part) => part.id === organizerId)?.name ?? null
      recordings.push({
        item: `https://${host}/_api/v2.1/drives/${encodeURIComponent(video.driveId)}`
          + `/items/${encodeURIComponent(video.driveItemId)}`,
        url: video.uri,
        title: textOf(raw.content, 'Title') ?? '',
        startedAt: content.timestamp ?? null,
        duration: content.duration ?? null,
        organizer: call?.organizerUpn ?? organizerName ?? '',
        participants: (call?.participants ?? []).map((part) => part.name).filter(Boolean),
        hasTranscript: (content.contentTypes ?? '').includes('Transcript'),
      })
    }
    return { signedIn: true, found: true, recordings }
  }

  // The freshest access token Teams holds for `host`, or why there is none.
  function tokenFor(host) {
    const nowSeconds = Date.now() / 1000
    let best = null
    let sawExpired = false
    for (let index = 0; index < localStorage.length; index++) {
      const key = localStorage.key(index) ?? ''
      if (!key.toLowerCase().includes('accesstoken')) {
        continue
      }
      let entry = null
      try {
        entry = JSON.parse(localStorage.getItem(key) ?? 'null')
      }
      catch {
        continue
      }
      const isForHost = typeof entry?.target === 'string'
        && entry.target.includes(`https://${host}/`)
        && typeof entry?.secret === 'string'
      if (!isForHost) {
        continue
      }
      const expiresOn = Number(entry.expiresOn ?? entry.expires_on)
      if (Number.isFinite(expiresOn) && expiresOn <= nowSeconds + EXPIRY_MARGIN_SECONDS) {
        sawExpired = true
        continue
      }
      if (!best || expiresOn > best.expiresOn) {
        best = { secret: entry.secret, expiresOn }
      }
    }
    if (best) {
      return { token: best.secret }
    }
    const error = sawExpired
      ? 'token_expired'
      : 'token_not_found'
    return { error }
  }

  async function fetchRecording(item) {
    const address = URL.canParse(item)
      ? new URL(item)
      : null
    const isDriveItem = address?.protocol === 'https:'
      && SHAREPOINT_HOST.test(address.host)
      && /^\/_api\/v2\.1\/drives\/[^/]+\/items\/[^/]+$/.test(address.pathname)
      && !address.search
    if (!isDriveItem) {
      return { signedIn: true, ok: false, error: 'not_a_drive_item' }
    }
    const { token, error } = tokenFor(address.host)
    if (!token) {
      return { signedIn: true, ok: false, error }
    }
    // One GET as the page, answering its JSON, or the fetch's refusal naming `step`.
    async function get(url, step) {
      const response = await fetch(url, {
        headers: { Authorization: `Bearer ${token}`, Accept: 'application/json' },
      })
      if (!response.ok) {
        return {
          refusal: {
            signedIn: response.status !== 401,
            ok: false,
            error: 'http_error',
            status: response.status,
            step,
          },
        }
      }
      return { body: await response.json() }
    }

    const metadata = await get(`${item}?select=id,file,video,audio,@content.downloadUrl`, 'item')
    if (metadata.refusal) {
      return metadata.refusal
    }
    // Absent when the organizer blocked downloads; the transcript is still read.
    const downloadUrl = metadata.body['@content.downloadUrl']
    const recording = typeof downloadUrl === 'string'
      ? {
        url: downloadUrl,
        mimeType: metadata.body.file?.mimeType ?? null,
        isVideo: Boolean(metadata.body.video),
      }
      : null

    const listing = await get(`${item}/media/transcripts`, 'transcripts')
    if (listing.refusal) {
      return listing.refusal
    }
    const transcripts = Array.isArray(listing.body.value)
      ? listing.body.value
      : []
    const chosen = transcripts.find((entry) => entry.isDefault) ?? transcripts[0]
    if (!chosen?.id) {
      return { signedIn: true, ok: true, recording, transcript: null }
    }
    const transcriptId = encodeURIComponent(chosen.id)
    const content = await get(`${item}/media/transcripts/${transcriptId}/content?format=json`, 'transcript')
    if (content.refusal) {
      return content.refusal
    }
    const entries = Array.isArray(content.body.entries)
      ? content.body.entries
      : []
    return {
      signedIn: true,
      ok: true,
      recording,
      transcript: entries.map((entry) => ({
        start: entry.startOffset ?? null,
        end: entry.endOffset ?? null,
        speaker: entry.speakerDisplayName ?? '',
        text: entry.text ?? '',
      })),
    }
  }

  return (async () => {
    if (/login\.(microsoftonline|live)\.com/.test(location.hostname)) {
      return { signedIn: false, found: false, ok: false, recordings: [] }
    }
    if (request.action === 'list') {
      return list()
    }
    if (request.action === 'fetch') {
      return fetchRecording(request.item)
    }
    return { signedIn: true, ok: false, error: 'unknown_action' }
  })()
})
