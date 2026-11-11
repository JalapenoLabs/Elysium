// Copyright © 2026 Jalapeno Labs
//
// Calls Slack's own web API from Slack's signed-in tab, as Slack's web client does, and
// answers what it said. The API evaluates this over DevTools with its request as the argument
// on the last line, awaits it, and reads back
// `{ signedIn, teams: [{ id, name, url, userId, counts }], results: [...] }`. See
// docs/connectors.md, Slack.
//
// The web client keeps each signed-in workspace in `localStorage.localConfig_v2`, with the
// session token it sends as the `token` field of every API call. That token never leaves this
// page: only Slack's answers do. Only the read methods below can be called, so nothing here
// posts, reacts, or marks a conversation read.
(async (request) => {
  const READ_METHODS = new Set([
    'auth.test',
    'client.counts',
    'conversations.history',
    'conversations.info',
    // A huddle's transcript, which Slack keeps as a file of its own beside its AI notes.
    'files.info',
    'huddles.get',
    'huddles.history',
    'subscriptions.thread.getView',
    'users.conversations',
    'users.info',
  ])

  let config = null
  try {
    config = JSON.parse(localStorage.getItem('localConfig_v2') ?? 'null')
  }
  catch {
    config = null
  }

  // A workspace's API lives on its own Slack host. One stored elsewhere is not called, so a
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
    return { signedIn: false, teams: [], results: [] }
  }

  // The page is on app.slack.com and each workspace's API on its own host, so every call is
  // a credentialed cross-origin request. Slack answers one with this origin, rather than the
  // `*` a browser refuses alongside cookies, only when the request carries the web client's
  // own marker (`_x_gantry`); `slack_route` names the workspace, as the web client does.
  const addressOf = (team, method) => {
    const address = new URL(`api/${method}`, team.url)
    address.searchParams.set('_x_gantry', 'true')
    address.searchParams.set('slack_route', team.id)
    return address
  }

  const call = async (team, method, params) => {
    if (!READ_METHODS.has(method)) {
      return { ok: false, error: 'method_not_allowed' }
    }
    const body = new FormData()
    body.append('token', team.token)
    for (const [ key, value ] of Object.entries(params ?? {})) {
      body.append(key, String(value))
    }
    try {
      const response = await fetch(addressOf(team, method), {
        method: 'POST',
        body,
        credentials: 'include',
      })
      if (!response.ok) {
        return { ok: false, error: `http_${response.status}` }
      }
      return await response.json()
    }
    catch (error) {
      return { ok: false, error: 'fetch_failed', message: String(error) }
    }
  }

  const byId = new Map(teams.map((team) => [ team.id, team ]))
  const [ counted, results ] = await Promise.all([
    Promise.all(teams.map((team) => request.counts
      ? call(team, 'client.counts', {})
      : null)),
    Promise.all((request.calls ?? []).map((entry) => {
      const team = byId.get(entry.team)
      return team
        ? call(team, entry.method, entry.params)
        : { ok: false, error: 'team_not_signed_in' }
    })),
  ])

  return {
    signedIn: true,
    teams: teams.map((team, index) => ({
      id: team.id,
      name: team.name ?? '',
      url: team.url,
      userId: team.user_id ?? '',
      counts: counted[index],
    })),
    results,
  }
})(__ELYSIUM_REQUEST__)
