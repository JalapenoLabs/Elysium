// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `ActionItemState` in api/src/models/action_item.rs.
export const ACTION_ITEM_STATES = [ 'inbox', 'open', 'resolved', 'dismissed' ] as const
export type ActionItemState = typeof ACTION_ITEM_STATES[number]

// Mirrors `ActionItemPriority` in api/src/models/action_item.rs, most pressing first. Next
// ranks by this order.
export const ACTION_ITEM_PRIORITIES = [ 'urgent', 'high', 'normal', 'low' ] as const
export type ActionItemPriority = typeof ACTION_ITEM_PRIORITIES[number]

// Mirrors `Owner` in api/src/models/action_item.rs: whose list the item is on. Only the
// user's items appear in Next.
export type ActionItemOwner =
  | { kind: 'user' }
  | { kind: 'other', name: string }
  | { kind: 'nobody' }

// Mirrors `ActionItemResponse` in api/src/routes/v1/action_items/mod.rs.
export type ActionItem = {
  id: string
  title: string
  notes: string
  state: ActionItemState
  priority: ActionItemPriority
  dueAt: string | null
  // Hidden from Next until this moment passes.
  snoozedUntil: string | null
  // Someone else owes the next step: a name or an address.
  waitingOn: string | null
  owner: ActionItemOwner
  // Set exactly while the item is resolved.
  resolvedAt: string | null
  // Set exactly while the item is dismissed.
  dismissedAt: string | null
  // Set while the item is softly deleted; it can be restored.
  deletedAt: string | null
  projectIds: string[]
  // The initiatives it is in now, leaving out deleted ones.
  initiativeIds: string[]
  createdAt: string
  updatedAt: string
}

// Mirrors `CommentResponse` in api/src/routes/v1/action_items/mod.rs.
export type ActionItemComment = {
  id: string
  actionItemId: string
  // `user`, `elysia`, `session:<number>`, or `watcher:<provider>`. Only the user's own
  // comments can be edited or deleted here.
  author: string
  body: string
  createdAt: string
  updatedAt: string
}

// Mirrors `HistoryKind` in api/src/models/action_item_event.rs. Kinds are stored as text,
// so a later API can send one this build does not know; see `HistoryEntry.kind`.
export const HISTORY_KINDS = [
  'created',
  'updated',
  'state_changed',
  'deleted',
  'restored',
  'commented',
  'comment_edited',
  'comment_deleted',
  'project_added',
  'project_removed',
  'initiative_joined',
  'initiative_left',
  'link_added',
  'link_removed',
  'primary_link_changed',
  'link_write_cancelled',
  'pull_request_closed',
  'container_linked',
  'container_unlinked',
] as const
export type HistoryKind = typeof HISTORY_KINDS[number]

// Mirrors `HistoryEntryResponse` in api/src/routes/v1/action_items/mod.rs. An entry about an
// item joining or leaving an initiative names both, and shows in both histories.
export type HistoryEntry = {
  id: string
  actionItemId: string | null
  initiativeId: string | null
  // A `HistoryKind`, or a kind added by a later API.
  kind: string
  // `user`, `elysia`, `session:<number>`, or `watcher:<provider>`.
  actor: string
  // What changed; its shape follows `kind`, as docs/action-items.md lists.
  data: unknown
  createdAt: string
  // The changeset whose applying or undoing made the change, if one did.
  changesetId: string | null
}

// The API filters by state, project, initiative, waiting, and snoozed as well, but the
// frontend holds every live item in Redux and filters there, so only `deleted` is sent.
type ListActionItemsQuery = {
  // Deleted items only, instead of the rest.
  deleted?: boolean
}

type ListActionItemsResponse = {
  items: ActionItem[]
}

// Newest first.
export function listActionItems(query: ListActionItemsQuery = {}) {
  const searchParams = query.deleted
    ? { deleted: 'true' }
    : undefined
  return apiClient
    .get('v1/action-items', { searchParams })
    .json<ListActionItemsResponse>()
}

type ActionItemResponse = {
  item: ActionItem
}

// Answers deleted items too.
export function getActionItem(itemId: string) {
  return apiClient
    .get(`v1/action-items/${itemId}`)
    .json<ActionItemResponse>()
}

type CreateActionItemRequest = {
  title: string
  notes?: string
  priority?: ActionItemPriority
  dueAt?: string | null
  projectIds?: string[]
  initiativeIds?: string[]
}

// The item starts `open`: creating it accepts it.
export function createActionItem(body: CreateActionItemRequest) {
  return apiClient
    .post('v1/action-items', { json: body })
    .json<ActionItemResponse>()
}

// Omitted fields are left unchanged; `dueAt: null` removes the due date. Projects and
// initiatives change one at a time through their own routes below.
type UpdateActionItemRequest = {
  title?: string
  notes?: string
  priority?: ActionItemPriority
  dueAt?: string | null
}

export function updateActionItem(itemId: string, body: UpdateActionItemRequest) {
  return apiClient
    .patch(`v1/action-items/${itemId}`, { json: body })
    .json<ActionItemResponse>()
}

// Soft: the item can be restored.
export function deleteActionItem(itemId: string) {
  return apiClient
    .delete(`v1/action-items/${itemId}`)
}

export function restoreActionItem(itemId: string) {
  return apiClient
    .post(`v1/action-items/${itemId}/restore`)
    .json<ActionItemResponse>()
}

// Mirrors `Transition` in api/src/action_items/mod.rs: each is its own route, and a change
// the item's state does not allow answers 409.
export const ACTION_ITEM_TRANSITIONS = [ 'accept', 'resolve', 'dismiss', 'reopen' ] as const
export type ActionItemTransition = typeof ACTION_ITEM_TRANSITIONS[number]

export function transitionActionItem(itemId: string, transition: ActionItemTransition) {
  return apiClient
    .post(`v1/action-items/${itemId}/${transition}`)
    .json<ActionItemResponse>()
}

// `until` must be in the future; null ends the snooze.
export function snoozeActionItem(itemId: string, until: string | null) {
  return apiClient
    .post(`v1/action-items/${itemId}/snooze`, { json: { until }})
    .json<ActionItemResponse>()
}

// `on` is a name or an address up to 320 characters; null stops waiting.
export function waitOnActionItem(itemId: string, on: string | null) {
  return apiClient
    .post(`v1/action-items/${itemId}/wait`, { json: { on }})
    .json<ActionItemResponse>()
}

type ListHistoryResponse = {
  history: HistoryEntry[]
}

// Oldest first.
export function listActionItemHistory(itemId: string) {
  return apiClient
    .get(`v1/action-items/${itemId}/history`)
    .json<ListHistoryResponse>()
}

type ListCommentsResponse = {
  comments: ActionItemComment[]
}

// Oldest first.
export function listActionItemComments(itemId: string) {
  return apiClient
    .get(`v1/action-items/${itemId}/comments`)
    .json<ListCommentsResponse>()
}

type CommentResponse = {
  comment: ActionItemComment
}

export function createActionItemComment(itemId: string, body: string) {
  return apiClient
    .post(`v1/action-items/${itemId}/comments`, { json: { body }})
    .json<CommentResponse>()
}

// Only the author may edit a comment; anyone else's answers 409.
export function updateActionItemComment(itemId: string, commentId: string, body: string) {
  return apiClient
    .patch(`v1/action-items/${itemId}/comments/${commentId}`, { json: { body }})
    .json<CommentResponse>()
}

export function deleteActionItemComment(itemId: string, commentId: string) {
  return apiClient
    .delete(`v1/action-items/${itemId}/comments/${commentId}`)
}

// Membership routes are idempotent: adding what is there, or removing what is not, answers
// the item unchanged.

export function addActionItemProject(itemId: string, projectId: string) {
  return apiClient
    .put(`v1/action-items/${itemId}/projects/${projectId}`)
    .json<ActionItemResponse>()
}

export function removeActionItemProject(itemId: string, projectId: string) {
  return apiClient
    .delete(`v1/action-items/${itemId}/projects/${projectId}`)
    .json<ActionItemResponse>()
}

export function joinInitiative(itemId: string, initiativeId: string) {
  return apiClient
    .put(`v1/action-items/${itemId}/initiatives/${initiativeId}`)
    .json<ActionItemResponse>()
}

export function leaveInitiative(itemId: string, initiativeId: string) {
  return apiClient
    .delete(`v1/action-items/${itemId}/initiatives/${initiativeId}`)
    .json<ActionItemResponse>()
}

// Mirrors `LinkProvider` in api/src/models/action_item_link.rs.
export const LINK_PROVIDERS = [ 'jira', 'github' ] as const
export type LinkProvider = typeof LINK_PROVIDERS[number]

// Mirrors `LinkKind`: what an item links to. Jira links issues only.
export const LINK_KINDS = [ 'issue', 'pull-request' ] as const
export type LinkKind = typeof LINK_KINDS[number]

// Mirrors `LinkState`: the linked thing's state as its provider last reported it.
export const LINK_STATES = [ 'open', 'done', 'not-planned', 'merged', 'closed-unmerged' ] as const
export type LinkState = typeof LINK_STATES[number]

// Mirrors `LinkWriteKind`: a provider write a link owes.
export const LINK_WRITE_KINDS = [ 'close', 'comment' ] as const
export type LinkWriteKind = typeof LINK_WRITE_KINDS[number]

// Mirrors `PendingWriteResponse` in api/src/routes/v1/action_items/mod.rs.
export type PendingWrite = {
  id: string
  kind: LinkWriteKind
  // The comment it posts, for a comment.
  commentId: string | null
  // Zero until the watcher first tries it.
  attempts: number
  // The provider's last answer, or why the write waits.
  lastError: string | null
  lastAttemptAt: string | null
  createdAt: string
}

// Mirrors `ActionItemLinkResponse` in api/src/routes/v1/action_items/mod.rs.
export type ActionItemLink = {
  id: string
  actionItemId: string
  provider: LinkProvider
  kind: LinkKind
  credentialId: string
  // What a person reads: ELY-12, or owner/name#12.
  key: string
  url: string
  // As last read; the item page reads the provider live.
  title: string
  // Where comments are posted, and whose assignee owns the item.
  isPrimary: boolean
  state: LinkState
  // Whose it was when last read.
  owner: ActionItemOwner
  // Writes that have not landed, oldest first. A link with any is pending.
  pendingWrites: PendingWrite[]
  createdAt: string
  updatedAt: string
}

// Mirrors `Remote` in api/src/action_items/links/mod.rs: a linked thing as its provider
// reports it now.
export type LinkRemote = {
  key: string
  url: string
  title: string
  state: LinkState
  // The provider's own word: a Jira status, or open, closed, merged.
  status: string
  owner: ActionItemOwner
  // The assignee as the provider names them.
  assignee: string | null
  // The provider's own priority, such as Jira's High; GitHub has none.
  priority: string | null
  // A day, Jira only.
  dueDate: string | null
}

// One link read live: the remote, or why it could not be read.
export type LinkRemoteRead = {
  linkId: string
  remote: LinkRemote | null
  error: string | null
}

// What a link request names: a thing a picker listed and the credential that reaches it.
export type LinkTarget = {
  provider: LinkProvider
  credentialId: string
  kind: LinkKind
  // ELY-12, or owner/name#12.
  reference: string
}

type ListActionItemLinksResponse = {
  links: ActionItemLink[]
}

// The primary first, then oldest first.
export function listActionItemLinks(itemId: string) {
  return apiClient
    .get(`v1/action-items/${itemId}/links`)
    .json<ListActionItemLinksResponse>()
}

type ReadActionItemLinksResponse = {
  remotes: LinkRemoteRead[]
}

// Reads every link live, through its provider.
export function readActionItemLinkRemotes(itemId: string) {
  return apiClient
    .get(`v1/action-items/${itemId}/links/remote`)
    .json<ReadActionItemLinksResponse>()
}

type ActionItemLinkResponse = {
  link: ActionItemLink
}

// The thing is read through the credential first: outside a Jira allowlist answers 403, and
// a thing already linked to another item 409. The item's first link becomes its primary.
export function addActionItemLink(itemId: string, target: LinkTarget) {
  return apiClient
    .post(`v1/action-items/${itemId}/links`, { json: target })
    .json<ActionItemLinkResponse>()
}

// Removing the primary makes the oldest link left the primary.
export function removeActionItemLink(itemId: string, linkId: string) {
  return apiClient
    .delete(`v1/action-items/${itemId}/links/${linkId}`)
}

// Answers every link of the item, since the primary moved from one to another.
export function makePrimaryActionItemLink(itemId: string, linkId: string) {
  return apiClient
    .put(`v1/action-items/${itemId}/links/${linkId}/primary`)
    .json<ListActionItemLinksResponse>()
}

export function cancelActionItemLinkWrite(itemId: string, linkId: string, writeId: string) {
  return apiClient
    .delete(`v1/action-items/${itemId}/links/${linkId}/writes/${writeId}`)
}

type CreateActionItemFromLinkRequest = LinkTarget & {
  projectIds?: string[]
  initiativeIds?: string[]
}

type CreateActionItemFromLinkResponse = {
  item: ActionItem
  link: ActionItemLink
}

// Creates an item from a linked thing: its title, priority, due date, and owner come from
// the provider, and it starts open with the link as its primary.
export function createActionItemFromLink(body: CreateActionItemFromLinkRequest) {
  return apiClient
    .post('v1/action-items/from-link', { json: body })
    .json<CreateActionItemFromLinkResponse>()
}
