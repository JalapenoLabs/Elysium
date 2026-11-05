// Copyright © 2026 Jalapeno Labs

// Core
import { mutate } from 'swr'

// Misc
import { pageKey, PAGES_KEY } from '../api/routes/pageRoutes'
import { PAGE_CHANGES_WINDOW_MS } from '../constants'

const changedPageIds = new Set<string>()
let reloadTimer: ReturnType<typeof setTimeout> | null = null

// One change can touch many pages, and `page.changed` is announced once for each: deleting a
// branch, restoring it, or renumbering a parent's children. Loading each by id would fetch every
// one's text, on every client, for pages nobody has open. So the ids of a burst are gathered, and
// it ends in one load of the list, which holds no text, and a load of each page a view has open.
// SWR reloads a key only while a view holds it, so a client outside the Knowledge area asks nothing.
export function reloadChangedPage(pageId: string) {
  changedPageIds.add(pageId)
  reloadTimer ??= setTimeout(() => {
    reloadTimer = null
    void mutate(PAGES_KEY)
    for (const changedId of changedPageIds) {
      void mutate(pageKey(changedId))
    }
    changedPageIds.clear()
  }, PAGE_CHANGES_WINDOW_MS)
}
