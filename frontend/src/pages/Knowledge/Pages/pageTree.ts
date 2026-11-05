// Copyright © 2026 Jalapeno Labs

import type { KnowledgePageSummary } from '../../../api/routes/pageRoutes'

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
