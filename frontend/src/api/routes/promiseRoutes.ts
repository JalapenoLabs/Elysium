// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `PromiseState` in api/src/models/promise.rs.
export const PROMISE_STATES = [ 'waiting', 'due', 'drafted', 'kept', 'dismissed' ] as const
export type PromiseState = typeof PROMISE_STATES[number]

// Mirrors `MadeIn` in api/src/routes/v1/promises/mod.rs: the message a promise was made in, and
// whose timeline it is on, the person who made it and sends its follow-up.
export type PromiseMadeIn = {
  recordId: string
  sourceKind: string
  // Its conversation, as its title names it, such as `Reply in #launch in Mooreslab`.
  title: string
  url: string | null
  occurredAt: string
  personId: string
}

// Mirrors `PromiseResponse` in api/src/routes/v1/promises/mod.rs: something a person promised to
// tell someone once their project's work is done, which its Project Manager keeps. Named for the
// person's promise, since `Promise` is JavaScript's own.
export type PersonPromise = {
  id: string
  projectId: string
  state: PromiseState
  // Who it is for, by name.
  recipient: string
  // What was promised, as a short phrase.
  summary: string
  // `null` once the message it was made in is gone.
  madeIn: PromiseMadeIn | null
  // The work it waits on, at most one.
  workBatchId: string | null
  workItemId: string | null
  // When its work finished.
  dueAt: string | null
  // The item its follow-up is, once drafted.
  followUpItemId: string | null
  // Its follow-up's draft, once drafted, which only its person may read and send.
  draftId: string | null
  createdAt: string
}

type ListPromisesResponse = {
  promises: PersonPromise[]
}

// What every promise's SWR key starts with, which the event stream reloads them by.
export const PROMISES_KEY_PREFIX = 'v1/promises'

// The SWR key the promise an item follows up loads under.
export function followUpPromiseKey(itemId: string) {
  return `${PROMISES_KEY_PREFIX}?followUpItemId=${itemId}`
}

// The promise whose follow-up is the item, as a list of one or none.
export function listFollowUpPromises(itemId: string) {
  return apiClient
    .get(followUpPromiseKey(itemId))
    .json<ListPromisesResponse>()
}
