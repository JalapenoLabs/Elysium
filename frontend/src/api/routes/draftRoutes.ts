// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `ReplyPlace` in api/src/models/draft.rs: where a chat reply goes in its conversation.
export const REPLY_PLACES = [ 'thread', 'conversation' ] as const
export type ReplyPlace = typeof REPLY_PLACES[number]

// Mirrors `DraftState` in api/src/models/draft.rs.
export type DraftState = 'draft' | 'sending' | 'sent' | 'failed'

// Mirrors `DraftResponse` in api/src/routes/v1/drafts/mod.rs: a message the person sends as
// themselves, a draft until they send it.
export type Draft = {
  id: string
  // The record it answers, for a reply.
  replyToRecordId: string | null
  // What sends it: a connector for a chat reply, or a mailbox for mail.
  connectorId: string | null
  mailAccountId: string | null
  // Where it goes, in its sender's own terms. A mail's carries `to` and `subject`.
  target: Record<string, unknown>
  place: ReplyPlace | null
  body: string
  state: DraftState
  // Why the last send failed; set exactly while `failed`.
  lastError: string | null
  attemptedAt: string | null
  sentAt: string | null
  // The record the sent message became.
  sentRecordId: string | null
  // The promise it keeps, for a follow-up a Project Manager drafted, which its item's page sends
  // rather than the record's reply box.
  promiseId: string | null
  createdBy: string
  createdAt: string
  updatedAt: string
}

type DraftResponse = {
  draft: Draft
}

type ListDraftsResponse = {
  drafts: Draft[]
}

// The SWR key a record's unsent drafts load under.
export function recordDraftsKey(recordId: string) {
  return `v1/drafts?recordId=${recordId}`
}

// The person's drafts answering one record that were never sent, newest first.
export function listRecordDrafts(recordId: string) {
  return apiClient
    .get(recordDraftsKey(recordId))
    .json<ListDraftsResponse>()
}

export function getDraft(draftId: string) {
  return apiClient
    .get(`v1/drafts/${draftId}`)
    .json<DraftResponse>()
}

type CreateReplyRequest = {
  recordId: string
  // For a chat reply; mail takes none.
  place?: ReplyPlace
  // For a reply by mail, when the person has more than one mailbox.
  mailAccountId?: string
  body: string
}

// Writes a reply as a draft. Nothing is sent until `sendDraft`.
export function createReplyDraft(body: CreateReplyRequest) {
  return apiClient
    .post('v1/drafts', { json: { kind: 'reply', ...body }})
    .json<DraftResponse>()
}

export function updateDraft(draftId: string, body: string) {
  return apiClient
    .patch(`v1/drafts/${draftId}`, { json: { body }})
    .json<DraftResponse>()
}

export function deleteDraft(draftId: string) {
  return apiClient
    .delete(`v1/drafts/${draftId}`)
}

// Sends a draft as the person. A send that failed answers the draft `failed`, with the reason in
// `lastError` and its text kept, rather than an error.
export function sendDraft(draftId: string) {
  return apiClient
    .post(`v1/drafts/${draftId}/send`)
    .json<DraftResponse>()
}
