// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../../api/routes/meetingRoutes'
import type { KnowledgePageSummary, PagePlacement } from '../../../api/routes/pageRoutes'

// Misc
import { MEETINGS_PAGE_ID } from '../../../constants'
import { scoreSearchMatch, toSearchWords } from '../../../search/searchMatch'

// A meeting where the tree lists it. Meetings are no pages: each has its home under the Meetings
// page, and a linked entry in the Meetings group of every project it has a topic on, all opening its
// one page.
export type MeetingTreeEntry = {
  // Unique across the tree, since one meeting shows in several places.
  key: string
  meetingId: string
  // Empty when its source named none.
  title: string
}

// A project's meetings, newest first, as one group under the project's brief.
export type MeetingTreeGroup = {
  key: string
  projectId: string
  entries: MeetingTreeEntry[]
}

export type PageTree = {
  byId: Map<string, KnowledgePageSummary>
  // Each page's children in order, under its id, and the top of the tree under null. A page with
  // none has no entry.
  childrenByParent: Map<string | null, KnowledgePageSummary[]>
  // The meetings listed under a page, after its child pages: every meeting's home entry under the
  // Meetings page, newest first.
  meetingsByPage: Map<string, MeetingTreeEntry[]>
  // The Meetings group under a project's brief, after its child pages.
  meetingGroupsByPage: Map<string, MeetingTreeGroup>
  // On a filtered tree, the pages kept only to show where a match sits.
  contextIds?: Set<string>
}

// Nests the flat list `GET /pages` answers, in one pass. `summaries` arrive sorted by position,
// so each page's children keep that order. It lists no meetings; `withMeetingEntries` adds them.
export function buildPageTree(summaries: KnowledgePageSummary[]): PageTree {
  const byId = new Map<string, KnowledgePageSummary>()
  const childrenByParent = new Map<string | null, KnowledgePageSummary[]>()

  for (const summary of summaries) {
    byId.set(summary.id, summary)

    const siblings = childrenByParent.get(summary.parentId)
    if (siblings) {
      siblings.push(summary)
      continue
    }
    childrenByParent.set(summary.parentId, [ summary ])
  }

  return { byId, childrenByParent, meetingsByPage: new Map(), meetingGroupsByPage: new Map() }
}

// The tree with the reader's meetings in it: each one's home under the Meetings page, and a linked
// entry in a Meetings group under the brief of every project it has a topic on, in the order
// `meetings` holds them, newest first. Entries are never pages, so nothing moves, deletes, or drops
// onto one, and a page keeps its own children first.
export function withMeetingEntries(tree: PageTree, meetings: Meeting[]): PageTree {
  const briefIdByProject = new Map<string, string>()
  for (const page of tree.byId.values()) {
    if (page.briefProjectId) {
      briefIdByProject.set(page.briefProjectId, page.id)
    }
  }

  const homes: MeetingTreeEntry[] = []
  const meetingGroupsByPage = new Map<string, MeetingTreeGroup>()
  const unplacedProjectIds = new Set<string>()
  for (const meeting of meetings) {
    homes.push({ key: `meeting:${meeting.id}`, meetingId: meeting.id, title: meeting.title })

    for (const projectId of meeting.projectIds) {
      const briefId = briefIdByProject.get(projectId)
      if (!briefId) {
        unplacedProjectIds.add(projectId)
        continue
      }
      const entry = { key: `meeting:${projectId}:${meeting.id}`, meetingId: meeting.id, title: meeting.title }
      const group = meetingGroupsByPage.get(briefId)
      if (group) {
        group.entries.push(entry)
        continue
      }
      meetingGroupsByPage.set(briefId, { key: `meetings:${projectId}`, projectId, entries: [ entry ]})
    }
  }
  // A project deleted since its meetings loaded, or a tree loaded before the project was made.
  if (unplacedProjectIds.size && tree.byId.size) {
    console.debug('withMeetingEntries found meetings on projects with no brief in the tree', {
      projectIds: [ ...unplacedProjectIds ],
    })
  }

  const meetingsByPage = new Map<string, MeetingTreeEntry[]>()
  if (homes.length && tree.byId.has(MEETINGS_PAGE_ID)) {
    meetingsByPage.set(MEETINGS_PAGE_ID, homes)
  }
  return { ...tree, meetingsByPage, meetingGroupsByPage }
}

// Where a drag let go, as React Aria names it: on a row, or just before or after one.
export type PageDropTarget = {
  key: string
  dropPosition: 'before' | 'on' | 'after'
}

// A page another may move under, with the titles above it, since two pages can share a title.
export type PageDestination = {
  id: string
  title: string
  ancestorTitles: string[]
}

// What narrows the tree. Anything else it can be filtered by is one more field here and one more
// test in `filterPageTree`, and no view changes.
export type PageTreeFilter = {
  label: string | null
  // What was typed. Each word begins a word of the title, as the global search reads a title.
  title: string
}

export const NO_PAGE_TREE_FILTER: PageTreeFilter = { label: null, title: '' }

// The tree as `filter` leaves it: every page it matches, under the pages above it, so each still
// shows where it sits. It is a tree of the same shape, which `PageTree` draws as it does the whole
// one. With nothing to filter by, it is the tree itself.
export function filterPageTree(tree: PageTree, filter: PageTreeFilter) {
  const { label } = filter
  const titleWords = toSearchWords(filter.title)
  if (label === null && !titleWords.length) {
    return tree
  }

  const matchedIds = new Set<string>()
  const contextIds = new Set<string>()
  for (const page of tree.byId.values()) {
    if (label !== null && !page.labels.includes(label)) {
      continue
    }
    if (titleWords.length && !isTitleMatch(page.title, titleWords)) {
      continue
    }
    matchedIds.add(page.id)
    contextIds.delete(page.id)
    // The walk stops at a page already kept, since everything above it is too.
    let parent = page.parentId
      ? tree.byId.get(page.parentId)
      : undefined
    while (parent && !matchedIds.has(parent.id) && !contextIds.has(parent.id)) {
      contextIds.add(parent.id)
      parent = parent.parentId
        ? tree.byId.get(parent.parentId)
        : undefined
    }
  }

  // `byId` holds the pages in list order, so nesting them again keeps their order.
  const kept = [ ...tree.byId.values() ].filter((page) => matchedIds.has(page.id) || contextIds.has(page.id))
  return { ...buildPageTree(kept), contextIds }
}

// Whether two filtered trees show the same pages the same way, so one can stand for the other. The
// whole tree is never one of them: filtering it opens every branch, which is not how a person left
// them.
export function holdsSamePages(first: PageTree, second: PageTree) {
  if (!first.contextIds || !second.contextIds || first.byId.size !== second.byId.size) {
    return false
  }
  for (const [ id, page ] of first.byId) {
    if (second.byId.get(id) !== page || first.contextIds.has(id) !== second.contextIds.has(id)) {
      return false
    }
  }
  return true
}

function isTitleMatch(title: string, queryWords: string[]) {
  const text = { titleWords: toSearchWords(title), tagWords: [], parentWords: []}
  return scoreSearchMatch(queryWords, text) !== null
}

// Every label the pages carry, once each, in reading order.
export function collectLabelsInUse(summaries: KnowledgePageSummary[]) {
  const labels = new Set<string>()
  for (const summary of summaries) {
    for (const label of summary.labels) {
      labels.add(label)
    }
  }
  return [ ...labels ].sort((first, second) => first.localeCompare(second))
}

// The pages above this one, outermost first: its breadcrumb, and the branches to open so it
// shows in the tree. The API never lets a page sit under itself, but a page a client updated
// alone can disagree with a list loaded before a move, so a walk that comes back round stops.
export function findPageAncestors(tree: PageTree, pageId: string) {
  const ancestors: KnowledgePageSummary[] = []
  const seenIds = new Set([ pageId ])
  let parentId = tree.byId.get(pageId)?.parentId ?? null

  while (parentId) {
    const parent = tree.byId.get(parentId)
    if (!parent || seenIds.has(parentId)) {
      console.debug('findPageAncestors stopped short of the top of the tree', { pageId, parentId })
      break
    }
    seenIds.add(parentId)
    ancestors.unshift(parent)
    parentId = parent.parentId
  }

  return ancestors
}

// The children of `parentId` in order, as they would stand without `pageId`, which is the page
// moving: it never counts as a neighbour of where it lands.
export function findSiblingsBesides(tree: PageTree, parentId: string | null, pageId: string) {
  const children = tree.childrenByParent.get(parentId) ?? []
  return children.filter((child) => child.id !== pageId)
}

// Where a page sits now, as a move would name it, so putting it back is a move.
export function findPagePlacement(tree: PageTree, page: KnowledgePageSummary): PagePlacement {
  const siblings = tree.childrenByParent.get(page.parentId) ?? []
  const index = siblings.findIndex((sibling) => sibling.id === page.id)
  return {
    parentId: page.parentId,
    after: siblings[index - 1]?.id ?? null,
  }
}

// What a drop asks for, or null when it asks for nothing: the page is already there, or would end
// up under itself. React Aria refuses that second kind of target as the drag passes over it, but a
// page can move under another from someone else's change between the drag and the drop.
export function findDropPlacement(tree: PageTree, pageId: string, target: PageDropTarget) {
  const page = tree.byId.get(pageId)
  const targetPage = tree.byId.get(target.key)
  if (!page || !targetPage) {
    console.debug('findDropPlacement was given a page the tree no longer holds', { pageId, target })
    return null
  }

  // Just before or after itself, a page stays put, and it can't be named as its own neighbour.
  if (target.key === pageId && target.dropPosition !== 'on') {
    return null
  }

  const parentId = target.dropPosition === 'on'
    ? targetPage.id
    : targetPage.parentId
  if (parentId !== null && (parentId === pageId || isPageUnder(tree, parentId, pageId))) {
    console.debug('findDropPlacement refused a page under itself', { pageId, target })
    return null
  }

  const placement: PagePlacement = {
    parentId,
    after: findSiblingBefore(findSiblingsBesides(tree, parentId, pageId), target),
  }
  const current = findPagePlacement(tree, page)
  if (placement.parentId === current.parentId && placement.after === current.after) {
    return null
  }
  return placement
}

// The sibling a drop lands after, among siblings that leave out the page moving. Dropped on a row,
// the page goes last among that row's children.
function findSiblingBefore(siblings: KnowledgePageSummary[], target: PageDropTarget) {
  if (target.dropPosition === 'on') {
    return siblings.at(-1)?.id ?? null
  }
  if (target.dropPosition === 'after') {
    return target.key
  }
  const index = siblings.findIndex((sibling) => sibling.id === target.key)
  return siblings[index - 1]?.id ?? null
}

// Whether `pageId` sits anywhere under `ancestorId`.
export function isPageUnder(tree: PageTree, pageId: string, ancestorId: string) {
  return findPageAncestors(tree, pageId).some((ancestor) => ancestor.id === ancestorId)
}

// How many pages sit under this one, at any depth.
export function countPagesUnder(tree: PageTree, pageId: string) {
  let count = 0
  const waiting = [ pageId ]

  while (waiting.length) {
    const children = tree.childrenByParent.get(waiting.pop() ?? null) ?? []
    count += children.length
    waiting.push(...children.map((child) => child.id))
  }

  return count
}

// Every page `pageId` may move under, in the tree's order: all of them but itself and what is
// under it. One walk from the top, skipping that branch whole. A page not yet made, with no id, may
// go anywhere.
export function listPageDestinations(tree: PageTree, pageId: string | null) {
  const destinations: PageDestination[] = []

  function walk(parentId: string | null, ancestorTitles: string[]) {
    for (const child of tree.childrenByParent.get(parentId) ?? []) {
      if (child.id === pageId) {
        continue
      }
      destinations.push({ id: child.id, title: child.title, ancestorTitles })
      walk(child.id, [ ...ancestorTitles, child.title ])
    }
  }
  walk(null, [])

  return destinations
}
