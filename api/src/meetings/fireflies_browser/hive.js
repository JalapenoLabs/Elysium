// Copyright © 2026 Jalapeno Labs
//
// Asks Fireflies' own GraphQL endpoint, from Fireflies' signed-in tab, the queries the API hands
// it, as Fireflies' web app does, and answers what Fireflies said. The API evaluates this over
// DevTools with its request, `{ calls: [{ query, variables }] }`, as the argument on the last
// line, awaits it, and reads back `{ signedIn, results: [{ status, data, errors }] }`. See
// docs/meetings.md, Fireflies through a browser, and CLAUDE.md beside this file.
//
// The web app keeps its session token in `localStorage.AUTHORIZATION` and sends it as the
// `authorization` header of every call, beside its cookies. That token never leaves this page:
// only Fireflies' answers do. A call that is not a query, such as a mutation, is refused before
// it is sent, so nothing here changes anything in Fireflies.
(async (request) => {
  const ENDPOINT = '/api/v4/hive'

  let token = null
  try {
    token = localStorage.getItem('AUTHORIZATION')
  }
  catch {
    token = null
  }
  if (!token) {
    return { signedIn: false, results: [] }
  }

  // A document that names another kind of operation anywhere is refused whole.
  const isQuery = (query) => typeof query === 'string'
    && /^\s*query\b/.test(query)
    && !/\b(mutation|subscription)\b/.test(query)

  const ask = async (call) => {
    if (!isQuery(call?.query)) {
      return { status: 0, data: null, errors: [ { message: 'only queries are sent', code: 'not_a_query' } ] }
    }
    try {
      const response = await fetch(ENDPOINT, {
        method: 'POST',
        credentials: 'include',
        headers: { 'content-type': 'application/json', authorization: token },
        body: JSON.stringify({ query: call.query, variables: call.variables ?? {} }),
      })
      let body = null
      try {
        body = await response.json()
      }
      catch {
        body = null
      }
      const errors = Array.isArray(body?.errors)
        ? body.errors.map((error) => ({ message: String(error?.message ?? ''), code: error?.extensions?.code ?? null }))
        : []
      return { status: response.status, data: body?.data ?? null, errors }
    }
    catch (error) {
      return { status: 0, data: null, errors: [ { message: String(error), code: 'fetch_failed' } ] }
    }
  }

  // One at a time, as a person's own tab would, so a long listing never floods Fireflies.
  const results = []
  for (const call of request.calls ?? []) {
    results.push(await ask(call))
  }
  return { signedIn: true, results }
})(__ELYSIUM_REQUEST__)
