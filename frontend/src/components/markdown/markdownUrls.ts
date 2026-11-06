// Copyright © 2026 Jalapeno Labs

// Where a link or image in a page's Markdown points, read by the same parser the browser uses, so
// `//host`, `\\host`, and an absolute link back to Elysium all resolve as they would on a click. By
// the time these run, react-markdown's `urlTransform` has blanked any scheme it does not allow,
// such as `javascript:` and `data:`.

// Misc
import { API_BASE_PATH } from '../../constants'

export type MarkdownLinkTarget =
  // Within the document, such as a footnote: the browser scrolls to it.
  | { kind: 'fragment', href: string }
  // Elysium's own address, routed in the app.
  | { kind: 'internal', href: string }
  // A file Elysium's API serves, such as a page's attachment: no route of the app's, so the browser
  // follows it.
  | { kind: 'file', href: string }
  | { kind: 'external', href: string }
  // Nothing left to follow.
  | { kind: 'none' }

export type MarkdownImageSource =
  | { kind: 'internal', src: string }
  // Another origin, which the Content Security Policy refuses to load (`docs/security.md`).
  | { kind: 'remote', href: string, host: string }
  | { kind: 'none' }

function parseUrl(value: string) {
  try {
    return new URL(value, window.location.origin)
  }
  catch (error) {
    console.debug('A Markdown address could not be read', { value, error })
    return null
  }
}

export function readMarkdownLink(href: string | undefined): MarkdownLinkTarget {
  if (!href) {
    console.debug('A Markdown link has no address left to follow', { href })
    return { kind: 'none' }
  }
  if (href.startsWith('#')) {
    return { kind: 'fragment', href }
  }
  const url = parseUrl(href)
  if (!url) {
    return { kind: 'none' }
  }
  if (url.origin === window.location.origin) {
    const href = `${url.pathname}${url.search}${url.hash}`
    if (url.pathname.startsWith(`${API_BASE_PATH}/`)) {
      return { kind: 'file', href }
    }
    return { kind: 'internal', href }
  }
  return { kind: 'external', href: url.href }
}

export function readMarkdownImage(src: string | undefined): MarkdownImageSource {
  if (!src) {
    console.debug('A Markdown image has no source left to load', { src })
    return { kind: 'none' }
  }
  const url = parseUrl(src)
  if (!url) {
    return { kind: 'none' }
  }
  if (url.origin === window.location.origin) {
    return { kind: 'internal', src: `${url.pathname}${url.search}` }
  }
  return { kind: 'remote', href: url.href, host: url.host }
}
