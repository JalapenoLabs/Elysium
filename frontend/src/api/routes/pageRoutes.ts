// Copyright © 2026 Jalapeno Labs

import type { LearningAgent } from './learningRoutes'

// Misc
import { apiClient } from '../index'

// Mirrors `PageSummaryResponse` in api/src/routes/v1/pages/mod.rs: a page as the tree lists it,
// without what it says, so a big tree never carries its pages' bodies.
export type KnowledgePageSummary = {
  id: string
  // The page it sits under; null at the top of the tree.
  parentId: string | null
  // Its place among its siblings, lowest first. Only its order means anything.
  position: number
  title: string
  currentVersion: number
  // Set while it is deleted; it can be restored.
  deletedAt: string | null
}

// Mirrors `PageResponse`: one page whole.
export type KnowledgePage = KnowledgePageSummary & {
  // Markdown, read through `Markdown` (src/components/markdown/).
  body: string
  // Agents may only propose changes to it.
  proposalOnly: boolean
  // Who published the newest version, and when.
  publishedBy: string
  publishedAt: string
  // The agent that acted for them; null when they published it themselves.
  agent: LearningAgent | null
  createdBy: string
  createdAt: string
}

// The SWR keys `page.changed` revalidates, each only while a view holds it.
export const PAGES_KEY = 'v1/pages'

export function pageKey(pageId: string) {
  return `v1/pages/${pageId}`
}

type ListPagesResponse = {
  pages: KnowledgePageSummary[]
}

// Every live page, flat and in order among its siblings; `buildPageTree` nests them.
export function listPages() {
  return apiClient
    .get(PAGES_KEY)
    .json<ListPagesResponse>()
}

type GetPageResponse = {
  page: KnowledgePage
}

// One page whole, deleted or not.
export function getPage(pageId: string) {
  return apiClient
    .get(pageKey(pageId))
    .json<GetPageResponse>()
}
