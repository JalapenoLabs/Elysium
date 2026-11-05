// Copyright © 2026 Jalapeno Labs

import type { KnowledgePageSummary, PagePlacement } from '../../../api/routes/pageRoutes'

export type PageTree = {
  byId: Map<string, KnowledgePageSummary>
  // Each page's children in order, under its id, and the top of the tree under null. A page with
  // none has no entry.
  childrenByParent: Map<string | null, KnowledgePageSummary[]>
}

// Nests the flat list `GET /pages` answers, in one pass. `summaries` arrive sorted by position,
// so each page's children keep that order.
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

  return { byId, childrenByParent }
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
