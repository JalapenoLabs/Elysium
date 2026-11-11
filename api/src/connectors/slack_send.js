// Copyright © 2026 Jalapeno Labs
//
// Posts one message as the person, from Slack's signed-in tab, as Slack's web client does: one
// call to `chat.postMessage` on the workspace's own host, with the session the web client keeps.
// The API evaluates this over DevTools with its request as the argument on the last line, awaits
// it, and reads back `{ signedIn, ok, error?, channel?, ts?, teamUrl? }`. See docs/connectors.md,
// Sending.
//
// This is the only script that writes to Slack, and `chat.postMessage` is the only method it can
// call; the reader (slack.js) can call read methods alone. The session token never leaves this
// page: only Slack's answer does, without the message it echoes.
(async (request) => {
  // Long enough for Slack under load, and well inside the API's own wait, so a slow answer
  // fails here with a reason rather than as a timeout with none.
  const POST_TIMEOUT_MS = 15000

  let config = null
  try {
    config = JSON.parse(localStorage.getItem('localConfig_v2') ?? 'null')
  }
  catch {
    config = null
  }

  // A workspace's API lives on its own Slack host. One stored elsewhere is never called, so a
  // token is only ever sent back to Slack.
  const isSlackHost = (url) => {
    try {
      const parsed = new URL(url)
      return parsed.protocol === 'https:' && parsed.hostname.endsWith('.slack.com')
    }
    catch {
      return false
    }
  }
  const teams = Object.values(config?.teams ?? {})
    .filter((team) => team?.id && team?.token && isSlackHost(team?.url))
  if (!teams.length) {
    return { signedIn: false, ok: false, error: 'not_authed' }
  }
  const team = teams.find((candidate) => candidate.id === request.team)
  if (!team) {
    return { signedIn: true, ok: false, error: 'team_not_signed_in' }
  }

  const body = new FormData()
  body.append('token', team.token)
  body.append('channel', request.channel)
  body.append('text', request.text)
  if (request.threadTs) {
    body.append('thread_ts', request.threadTs)
  }
  // A credentialed call from app.slack.com to the workspace's own host: Slack answers it with
  // this origin, rather than the `*` a browser refuses alongside cookies, only when it carries
  // the web client's own marker (`_x_gantry`), as the reader's calls do (slack.js).
  const address = new URL('api/chat.postMessage', team.url)
  address.searchParams.set('_x_gantry', 'true')
  address.searchParams.set('slack_route', team.id)
  let answer = null
  try {
    const response = await fetch(address, {
      method: 'POST',
      body,
      credentials: 'include',
      signal: AbortSignal.timeout(POST_TIMEOUT_MS),
    })
    if (!response.ok) {
      return { signedIn: true, ok: false, error: `http_${response.status}` }
    }
    answer = await response.json()
  }
  catch (error) {
    return { signedIn: true, ok: false, error: 'fetch_failed', message: String(error) }
  }

  return {
    signedIn: true,
    ok: answer?.ok === true,
    error: answer?.error ?? null,
    channel: answer?.channel ?? null,
    ts: answer?.ts ?? null,
    teamUrl: team.url,
  }
})(__ELYSIUM_REQUEST__)
