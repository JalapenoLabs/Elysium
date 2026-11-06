// Copyright © 2026 Jalapeno Labs

// Core
import { mutate } from 'swr'

// Misc
import { pageHistoryKey, pageKey, PAGES_KEY } from '../api/routes/pageRoutes'
import { PAGE_CHANGES_WINDOW_MS } from '../constants'

const changedPageIds = new Set<string>()
let reloadTimer: ReturnType<typeof setTimeout> | null = null

// One change can touch many pages, and `page.changed` is announced once for each: deleting a
// branch, restoring it, or renumbering a parent's children. Loading each by id would fetch every
// one's text, on every client, for pages nobody has open. So the ids of a burst are gathered, and
// it ends in one load of the list, which holds no text, and a load of each page and history a view
// has open. The event names no project, and any page may have joined or left one or changed what a
// project lists of it, so each project's docs a view holds load once too.
// SWR reloads a key only while a view holds it, so a client outside these views asks nothing.
export function reloadChangedPage(pageId: string) {
  changedPageIds.add(pageId)
  reloadTimer ??= setTimeout(() => {
    reloadTimer = null
    void mutate(PAGES_KEY)
    void mutate((key) => typeof key === 'string'
      && key.startsWith('v1/projects/')
      && key.endsWith('/pages'))
    for (const changedId of changedPageIds) {
      void mutate(pageKey(changedId))
      void mutate(pageHistoryKey(changedId))
    }
    changedPageIds.clear()
  }, PAGE_CHANGES_WINDOW_MS)
}
