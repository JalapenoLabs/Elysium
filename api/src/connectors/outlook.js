// Copyright © 2026 Jalapeno Labs
//
// Reads the message list Outlook on the web shows, in its own signed-in tab, over DevTools.
// The API evaluates this as one expression and reads back
// `{ signedIn, messages: [{ id, from, subject, preview, unread }] }`. See docs/connectors.md.
//
// Outlook gives each conversation in the list a `data-convid` and an `aria-label` that reads
// it aloud ("Unread ... From ..."), and lays the row out as lines of text: the sender, the
// subject, the time, then the preview. Nothing here clicks or opens a message, so reading
// leaves unread mail unread.
(() => {
  const rows = [ ...document.querySelectorAll('[data-convid]') ]
  const messages = []
  const seen = new Set()
  for (const row of rows) {
    const id = row.getAttribute('data-convid')
    if (!id || seen.has(id)) {
      continue
    }
    seen.add(id)

    const label = row.getAttribute('aria-label') || ''
    const lines = (row.innerText || '')
      .split('\n')
      .map((line) => line.trim())
      .filter(Boolean)
    const address = [ ...row.querySelectorAll('[title*="@"]') ]
      .map((element) => element.getAttribute('title'))
      .find(Boolean)
    const sender = lines[0] || ''
    const from = address && !sender.includes(address)
      ? `${sender} <${address}>`
      : sender

    messages.push({
      id,
      from,
      subject: lines[1] || '',
      preview: lines.slice(3).join(' '),
      unread: /^unread\b/i.test(label) || /\bunread\b/i.test(label.slice(0, 40)),
    })
  }
  return { signedIn: true, messages }
})()
