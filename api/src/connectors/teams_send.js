// Copyright © 2026 Jalapeno Labs
//
// Posts one message as the person, from Microsoft Teams' own signed-in tab, through the chat
// service the web client posts what its person types to. The API evaluates this over DevTools
// with its request as the argument on the last line, awaits it, and reads back
// `{ signedIn, ok, error?, status?, arrivalTime? }`. See docs/connectors.md, Sending.
//
// The chat service is the web client's own, not a published API, so everything it needs is
// found defensively and a missing piece is named rather than guessed:
//
// - The token is the web client's access token for `ic3.teams.office.com`, from the MSAL cache
//   it keeps in localStorage (entries whose key holds `accesstoken`, `{ secret, target,
//   expiresOn }`). One that has expired is not used (`token_expired`); none at all is
//   `token_not_found`, which is also what an encrypted cache looks like.
// - The chat service's address, with its region, is taken from a call the web client already
//   made (`/api/chatsvc/<region>/`), from the page's own resource timings, and only on a Teams
//   host. None seen is `region_unknown`.
//
// The token never leaves this page: only whether the post succeeded does.
(async (request) => {
  // Long enough for the chat service under load, and well inside the API's own wait, so a slow
  // answer fails here with a reason rather than as a timeout with none.
  const POST_TIMEOUT_MS = 15000
  // A token this close to expiring could lapse on the way.
  const EXPIRY_MARGIN_SECONDS = 60
  const TEAMS_HOSTS = new Set([ 'teams.cloud.microsoft', 'teams.microsoft.com', 'teams.live.com' ])

  if (/login\.(microsoftonline|live)\.com/.test(location.hostname)) {
    return { signedIn: false, ok: false, error: 'signed_out' }
  }

  const nowSeconds = Date.now() / 1000
  let token = null
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
    const isChatToken = typeof entry?.target === 'string'
      && entry.target.includes('ic3.teams.office.com')
      && typeof entry?.secret === 'string'
    if (!isChatToken) {
      continue
    }
    const expiresOn = Number(entry.expiresOn ?? entry.expires_on)
    if (Number.isFinite(expiresOn) && expiresOn <= nowSeconds + EXPIRY_MARGIN_SECONDS) {
      sawExpired = true
      continue
    }
    token = entry.secret
    break
  }
  if (!token) {
    return {
      signedIn: true,
      ok: false,
      error: sawExpired
        ? 'token_expired'
        : 'token_not_found',
    }
  }

  // `https://<teams host>/api/chatsvc/<region>`, as the web client last called it.
  let base = null
  for (const timing of performance.getEntriesByType('resource')) {
    const match = /^(https:\/\/([^/]+)\/api\/chatsvc\/[a-z]+)\//.exec(timing.name)
    if (match && TEAMS_HOSTS.has(match[2])) {
      base = match[1]
    }
  }
  if (!base) {
    return { signedIn: true, ok: false, error: 'region_unknown' }
  }

  // A reply in a channel thread posts to the thread's first message; anything else posts to the
  // conversation itself.
  const conversation = request.rootMessageId
    ? `${request.conversationId};messageid=${request.rootMessageId}`
    : request.conversationId
  const escaped = request.text
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll('\n', '<br>')

  let response = null
  try {
    response = await fetch(`${base}/v1/users/ME/conversations/${encodeURIComponent(conversation)}/messages`, {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        content: `<p>${escaped}</p>`,
        messagetype: 'RichText/Html',
        contenttype: 'text',
        clientmessageid: String(Date.now()),
        imdisplayname: request.senderName,
        properties: { importance: '', subject: null },
      }),
      signal: AbortSignal.timeout(POST_TIMEOUT_MS),
    })
  }
  catch (error) {
    return { signedIn: true, ok: false, error: 'fetch_failed', message: String(error) }
  }
  if (!response.ok) {
    return {
      signedIn: response.status !== 401,
      ok: false,
      error: 'http_error',
      status: response.status,
    }
  }

  let answer = null
  try {
    answer = await response.json()
  }
  catch {
    answer = null
  }
  return { signedIn: true, ok: true, arrivalTime: answer?.OriginalArrivalTime ?? null }
})(__ELYSIUM_REQUEST__)
