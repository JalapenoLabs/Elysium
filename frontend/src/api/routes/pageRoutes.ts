// Copyright © 2026 Jalapeno Labs

import type { KnowledgePassage } from './knowledgeRoutes'
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
  // The project whose brief it is, which it stays linked to; null for every other page.
  briefProjectId: string | null
  // The labels it carries, in order. Holding them here filters the tree, lists a label's pages, and
  // suggests the labels in use, all from what the tree already loaded.
  labels: string[]
  // How many edits agents suggested to it wait for a person, which the tree shows.
  pendingSuggestions: number
  // When what it holds last changed: published, deleted, or restored. A move or a label leaves it.
  updatedAt: string
  // Set while it is deleted; it can be restored.
  deletedAt: string | null
}

// Mirrors `AttachmentResponse`: a file on a page.
export type PageAttachment = {
  id: string
  pageId: string
  name: string
  // Where it is served, which a page's Markdown refers to it by.
  url: string
  sizeBytes: number
  // Whether it displays in a page; anything else is a link that downloads. The API decides.
  isImage: boolean
  // Who added it, and the agent that acted for them.
  createdBy: string
  agent: LearningAgent | null
  createdAt: string
}

// Mirrors `PageResponse`: one page whole.
export type KnowledgePage = KnowledgePageSummary & {
  // Markdown, read through `Markdown` (src/components/markdown/).
  body: string
  // The files it holds, the oldest first. No part of a version.
  attachments: PageAttachment[]
  // What a learning holds beyond its text; null on every other page.
  category: string | null
  evidence: string | null
  // Agents may only suggest edits to it.
  proposalOnly: boolean
  // The Company page or one of the company doc's sections, whose review a person can start.
  companyDoc: boolean
  // The projects it is linked to, oldest link first. A link is no version, and changes nobody's
  // access.
  projectIds: string[]
  // Who published the newest version, and when.
  publishedBy: string
  publishedAt: string
  // The agent that acted for them; null when they published it themselves.
  agent: LearningAgent | null
  createdBy: string
  createdAt: string
}

// Mirrors `ProjectPageResponse` in api/src/routes/v1/projects/list_pages.rs: a live page as the
// project linked to it lists it.
export type ProjectPage = KnowledgePageSummary & {
  // Who published the newest version, and when.
  publishedBy: string
  publishedAt: string
  // Who linked it to the project, and when.
  linkedBy: string
  linkedAt: string
}

// Mirrors `HistoryEntryResponse`: one thing that happened to a page, publishes included.
export type PageHistoryEntry = {
  id: string
  pageId: string
  // `created`, `published`, `moved`, `deleted`, `restored`, `labeled`, `unlabeled`,
  // `project_linked`, or `project_unlinked`; a later API may write others.
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

// What a publish did: a new version, or nothing, for text the page already holds.
export type PublishOutcome = 'published' | 'unchanged'

// The API's own bounds on a page, which it checks after normalizing.
export const PAGE_TITLE_MAX_CHARACTERS = 300
export const PAGE_BODY_MAX_CHARACTERS = 100_000

// The SWR keys `page.changed` revalidates, each only while a view holds it.
export const PAGES_KEY = 'v1/pages'

// A passage of a page, as of one version: publishing can recut its text.
export function pagePassageKey(pageId: string, position: number, version: number) {
  return [ 'page-passage', pageId, position, version ] as const
}

export function pageKey(pageId: string) {
  return `v1/pages/${pageId}`
}

export function pageHistoryKey(pageId: string) {
  return `${pageKey(pageId)}/history`
}

export function projectPagesKey(projectId: string) {
  return `v1/projects/${projectId}/pages`
}

export function pageSuggestionsKey(pageId: string) {
  return `${pageKey(pageId)}/suggestions`
}

// Mirrors `ChangeResponse`: one change a suggestion makes, numbered from 0 as accepting it in part
// names it.
export type PageSuggestionChange = {
  index: number
  // Its first line in the version the suggestion was written from, from 1.
  line: number
  // The lines it changes, with a few unchanged ones on each side, before and after it.
  before: string
  after: string
  // Whether accepting it alone would merge with the page now: false when the page changed since,
  // in the same lines.
  applies: boolean
}

// Mirrors `SuggestionResponse`: an edit an agent suggested, read as its changes.
export type PageSuggestion = {
  id: string
  pageId: string
  // The version it was written from.
  baseVersion: number
  changes: PageSuggestionChange[]
  reason: string
  quote: string | null
  source: string | null
  // The company doc review's run that suggested it, while that run exists.
  workflowRunId: string | null
  state: 'pending' | 'accepted' | 'dismissed'
  // The changes an acceptance took; null for all of them.
  acceptedChanges: number[] | null
  dismissalReason: string | null
  decidedBy: string | null
  decidedAt: string | null
  // A person, or the machine user of Elysia or a Project Manager.
  createdBy: string
  agent: LearningAgent | null
  createdAt: string
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
type GetPagePassageResponse = {
  passage: KnowledgePassage
}

// One passage of a page as the index cut it, which a reader opened at it finds in the page's
// Markdown. A page, or a place in it, the index no longer holds answers `404`.
export function getPagePassage(pageId: string, position: number) {
  return apiClient
    .get(`v1/pages/${pageId}/passages/${position}`)
    .json<GetPagePassageResponse>()
}

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
  outcome: PublishOutcome
}

// Publishes a version's content as the page's next version, naming the one it copied.
export function restorePageVersion(pageId: string, version: number) {
  return apiClient
    .post(`${pageVersionKey(pageId, version)}/restore`)
    .json<RestorePageVersionResponse>()
}

// Mirrors `MovePageRequest`: the page it goes under, or null for the top of the tree, and the
// sibling it follows, or null to go first.
export type PagePlacement = {
  parentId: string | null
  after: string | null
}

type MovePageResponse = {
  page: KnowledgePage
}

// Moves a page with everything under it, or reorders it among its siblings.
export function movePage(pageId: string, placement: PagePlacement) {
  return apiClient
    .post(`${pageKey(pageId)}/move`, { json: placement })
    .json<MovePageResponse>()
}

type DeletePageOptions = {
  // The page goes alone, and its children take its place.
  keepChildren: boolean
}

// Deletes a page softly, with every page under it unless its children are kept.
export async function deletePage(pageId: string, options: DeletePageOptions) {
  await apiClient.delete(pageKey(pageId), {
    searchParams: options.keepChildren
      ? { keepChildren: true }
      : undefined,
  })
}

type RestorePageResponse = {
  page: KnowledgePage
}

// Brings a deleted page back where it was, with what its deletion took or moved out of it.
export function restorePage(pageId: string) {
  return apiClient
    .post(`${pageKey(pageId)}/restore`)
    .json<RestorePageResponse>()
}

type CreatePageRequest = {
  // The page it goes under; left out, the top of the tree.
  parentId?: string
  title: string
  body: string
}

type CreatePageResponse = {
  page: KnowledgePage
}

export function createPage(body: CreatePageRequest) {
  return apiClient
    .post(PAGES_KEY, { json: body })
    .json<CreatePageResponse>()
}

type PublishPageRequest = {
  // The version this was edited from; a newer one is refused with `409 page_changed`.
  baseVersion: number
  title: string
  body: string
  // A learning's, both or neither.
  category?: string
  evidence?: string
}

type PublishPageResponse = {
  page: KnowledgePage
  outcome: PublishOutcome
}

export function publishPage(pageId: string, body: PublishPageRequest) {
  return apiClient
    .post(`${pageKey(pageId)}/publish`, { json: body })
    .json<PublishPageResponse>()
}

// Puts a label on a page, however it is typed: the API reads `On Call` as the label `on-call`.
export function addPageLabel(pageId: string, label: string) {
  return apiClient
    .post(`${pageKey(pageId)}/labels`, { json: { label }})
    .json<GetPageResponse>()
}

export function removePageLabel(pageId: string, label: string) {
  return apiClient
    .delete(`${pageKey(pageId)}/labels/${encodeURIComponent(label)}`)
    .json<GetPageResponse>()
}

type AttachPageFileResponse = {
  attachment: PageAttachment
  page: KnowledgePage
}

// Attaches a file. The body is the file itself, and the API takes what it is from its name.
export function attachPageFile(pageId: string, file: File) {
  return apiClient
    .post(`${pageKey(pageId)}/attachments`, {
      searchParams: { name: file.name },
      body: file,
      // A large file on a slow connection takes as long as it takes, and its row says it is on its
      // way, so it needs neither the usual timeout nor the notice that a request is running long.
      timeout: false,
      fetch: (input, init) => globalThis.fetch(input, init),
    })
    .json<AttachPageFileResponse>()
}

// Takes a file off a page; `restorePageAttachment` puts it back.
export function removePageAttachment(pageId: string, attachmentId: string) {
  return apiClient
    .delete(`${pageKey(pageId)}/attachments/${attachmentId}`)
    .json<GetPageResponse>()
}

export function restorePageAttachment(pageId: string, attachmentId: string) {
  return apiClient
    .post(`${pageKey(pageId)}/attachments/${attachmentId}/restore`)
    .json<GetPageResponse>()
}

// Mirrors `PolicyResponse`: what can be attached, which the API holds as constants.
export type PageAttachmentPolicy = {
  // The storage location new attachments go to; null until an admin chooses one.
  destinationId: string | null
  maxBytes: number
  // Lowercase, without their dots.
  extensions: string[]
}

// The SWR key `pageAttachments.changed` revalidates.
export const PAGE_ATTACHMENT_POLICY_KEY = 'v1/page-attachments'

type PageAttachmentPolicyResponse = {
  policy: PageAttachmentPolicy
}

export function getPageAttachmentPolicy() {
  return apiClient
    .get(PAGE_ATTACHMENT_POLICY_KEY)
    .json<PageAttachmentPolicyResponse>()
}

// Admins only. Null stops taking attachments; what is attached stays served.
export function choosePageAttachmentDestination(storageLocationId: string | null) {
  return apiClient
    .put(`${PAGE_ATTACHMENT_POLICY_KEY}/destination`, { json: { storageLocationId }})
    .json<PageAttachmentPolicyResponse>()
}

type ListProjectPagesResponse = {
  pages: ProjectPage[]
}

type PageSuggestionsResponse = {
  suggestions: PageSuggestion[]
}

// The suggestions waiting on a page, oldest first.
export function listPageSuggestions(pageId: string) {
  return apiClient
    .get(pageSuggestionsKey(pageId))
    .json<PageSuggestionsResponse>()
}

type DecidedSuggestionResponse = {
  suggestion: PageSuggestion
  page: KnowledgePage
}

// Every change, or those `changes` names by their index. One whose lines the page changed since
// answers 409 with the code `suggestion_overlaps`, and the suggestion stays pending.
export function acceptPageSuggestion(pageId: string, suggestionId: string, changes?: number[]) {
  return apiClient
    .post(`${pageSuggestionsKey(pageId)}/${suggestionId}/accept`, { json: { changes }})
    .json<DecidedSuggestionResponse>()
}

export function dismissPageSuggestion(pageId: string, suggestionId: string, reason: string) {
  return apiClient
    .post(`${pageSuggestionsKey(pageId)}/${suggestionId}/dismiss`, { json: { reason }})
    .json<DecidedSuggestionResponse>()
}

// The live pages linked to a project, its docs, newest published first; members alone.
export function listProjectPages(projectId: string) {
  return apiClient
    .get(projectPagesKey(projectId))
    .json<ListProjectPagesResponse>()
}

export function linkPageToProject(pageId: string, projectId: string) {
  return apiClient
    .put(`${pageKey(pageId)}/projects/${projectId}`)
    .json<GetPageResponse>()
}

export function unlinkPageFromProject(pageId: string, projectId: string) {
  return apiClient
    .delete(`${pageKey(pageId)}/projects/${projectId}`)
    .json<GetPageResponse>()
}
