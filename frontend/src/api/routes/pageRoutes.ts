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
  // One of the pages Elysium relies on, which nobody moves, deletes, or renames.
  fixed: boolean
  // Set while it is deleted; it can be restored.
  deletedAt: string | null
}

// Mirrors `PageResponse`: one page whole.
export type KnowledgePage = KnowledgePageSummary & {
  // Markdown, read through `Markdown` (src/components/markdown/).
  body: string
  // What a learning holds beyond its text; null on every other page.
  category: string | null
  evidence: string | null
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

// Mirrors `HistoryEntryResponse`: one thing that happened to a page, publishes included.
export type PageHistoryEntry = {
  id: string
  pageId: string
  // `created`, `published`, `moved`, `deleted`, or `restored`; a later API may write others.
  kind: string
  // Who made the change, and the agent that acted for them.
  createdBy: string
  agent: LearningAgent | null
  // Shaped by `kind` (docs/knowledge.md, Pages, History); `describePageHistory` reads it.
  data: Record<string, unknown>
  createdAt: string
}

// Mirrors `VersionResponse`: one version whole.
export type PageVersion = {
  pageId: string
  version: number
  title: string
  // The version a restore copied; null for one published as written.
  restoredFrom: number | null
  // Who published it, and the agent that acted for them.
  createdBy: string
  agent: LearningAgent | null
  createdAt: string
  // Markdown.
  body: string
  // What a learning held then; null on every other page.
  category: string | null
  evidence: string | null
}

// The SWR keys `page.changed` revalidates, each only while a view holds it.
export const PAGES_KEY = 'v1/pages'

export function pageKey(pageId: string) {
  return `v1/pages/${pageId}`
}

export function pageHistoryKey(pageId: string) {
  return `${pageKey(pageId)}/history`
}

// A version never changes once written, so nothing revalidates this key.
export function pageVersionKey(pageId: string, version: number) {
  return `${pageKey(pageId)}/versions/${version}`
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

type ListPageHistoryResponse = {
  history: PageHistoryEntry[]
}

// Everything that happened to a page, oldest first, without what any version says, so a long
// history stays light however large the page is.
export function listPageHistory(pageId: string) {
  return apiClient
    .get(pageHistoryKey(pageId))
    .json<ListPageHistoryResponse>()
}

type GetPageVersionResponse = {
  version: PageVersion
}

export function getPageVersion(pageId: string, version: number) {
  return apiClient
    .get(pageVersionKey(pageId, version))
    .json<GetPageVersionResponse>()
}

type RestorePageVersionResponse = {
  page: KnowledgePage
  // `unchanged` when the page already says exactly this, and no version was made.
  outcome: 'published' | 'unchanged'
}

// Publishes a version's content as the page's next version, naming the one it copied.
export function restorePageVersion(pageId: string, version: number) {
  return apiClient
    .post(`${pageVersionKey(pageId, version)}/restore`)
    .json<RestorePageVersionResponse>()
}
