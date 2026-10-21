// Copyright © 2026 Jalapeno Labs
//
// Reads the unread mail in Gmail's inbox, in its own signed-in tab, over DevTools. The API
// evaluates this as one expression, awaits it, and reads back
// `{ signedIn, messages: [{ id, from, subject, preview, unread, receivedAt, url }] }`. See
// docs/connectors.md.
//
// Gmail publishes the inbox's unread messages as an Atom feed at /mail/u/<n>/feed/atom, for
// the account the tab shows. Fetching it from the tab sends the tab's own session, so this is
// the person reading their own feed. It lists unread mail only, and reading it marks nothing.
(async () => {
  const account = location.pathname.match(/\/mail\/u\/(\d+)/)?.[1] ?? '0'
  const response = await fetch(`${location.origin}/mail/u/${account}/feed/atom`, { credentials: 'include' })
  if (response.status === 401 || response.status === 403) {
    return { signedIn: false, messages: [] }
  }
  if (!response.ok) {
    throw new Error(`Gmail's feed answered ${response.status}`)
  }

  const feed = new DOMParser().parseFromString(await response.text(), 'application/xml')
  const text = (parent, selector) => parent.querySelector(selector)?.textContent?.trim() ?? ''
  const messages = [ ...feed.querySelectorAll('entry') ].map((entry) => {
    const name = text(entry, 'author > name')
    const email = text(entry, 'author > email')
    const from = email && name
      ? `${name} <${email}>`
      : name || email
    return {
      // `tag:gmail.google.com,2004:1812345678901234567`: the message's own id.
      id: text(entry, 'id').split(':').pop(),
      from,
      subject: text(entry, 'title'),
      preview: text(entry, 'summary'),
      unread: true,
      receivedAt: text(entry, 'issued') || text(entry, 'modified') || null,
      url: entry.querySelector('link')?.getAttribute('href') ?? null,
    }
  })
  return { signedIn: true, messages }
})()
