// Copyright © 2026 Jalapeno Labs

import type { MeetingComment, MeetingMark } from '../../../api/routes/meetingRoutes'
import type { User } from '../../../api/routes/userRoutes'
import type { PlacedMark } from './meetingTranscript'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { ELYSIA_USER_ID } from '../../../constants'
import {
  awaitsElysia,
  keptMentions,
  mentionAt,
  mentionCandidates,
  mentionPieces,
  threadsOf,
} from './meetingThreads'

function placed(id: string, kind: PlacedMark['kind'], start: number, data: Record<string, unknown> = {}): PlacedMark {
  const mark: MeetingMark = {
    id,
    kind,
    startWordId: `w${start}`,
    endWordId: `w${start}`,
    data,
    createdBy: 'ada',
    agent: null,
    createdAt: '2026-11-10T12:00:00Z',
    excerpt: null,
  }
  return { mark, kind, start, end: start }
}

function comment(id: string, threadId: string, createdBy: string, mentions: string[] = []): MeetingComment {
  return {
    id,
    threadId,
    body: `comment ${id}`,
    mentions,
    createdBy,
    agent: null,
    createdAt: '2026-11-10T12:00:00Z',
  }
}

function user(id: string, name: string, overrides: Partial<User> = {}): User {
  return {
    id,
    kind: 'person',
    name,
    email: null,
    role: 'member',
    status: 'active',
    approvedAt: null,
    lastSeenAt: null,
    createdAt: '2026-11-10T12:00:00Z',
    updatedAt: '2026-11-10T12:00:00Z',
    ...overrides,
  }
}

describe('threadsOf', () => {
  it('gathers each thread\'s comments in order, open and resolved apart, in transcript order', () => {
    const marks = [
      placed('first', 'comment', 0),
      placed('flag', 'confidence', 1),
      placed('done', 'comment', 2, { resolved: true }),
      placed('later', 'comment', 3, { resolved: false }),
    ]
    const comments = [
      comment('a', 'first', 'ada'),
      comment('b', 'done', 'grace'),
      comment('c', 'first', 'grace'),
      comment('gone', 'removed-thread', 'ada'),
    ]

    const threads = threadsOf(marks, comments)

    expect(threads.open.map((thread) => thread.placed.mark.id)).toEqual([ 'first', 'later' ])
    expect(threads.open[0].comments.map((written) => written.id)).toEqual([ 'a', 'c' ])
    expect(threads.open[1].comments).toEqual([])
    expect(threads.resolved.map((thread) => thread.placed.mark.id)).toEqual([ 'done' ])
    expect(threads.resolved[0].isResolved).toBe(true)
  })
})

describe('awaitsElysia', () => {
  it('waits on her from a comment that mentions her until she writes after it', () => {
    const asked = comment('a', 't', 'ada', [ ELYSIA_USER_ID ])
    const answered = comment('b', 't', ELYSIA_USER_ID)
    const later = comment('c', 't', 'ada')

    expect(awaitsElysia([ asked ])).toBe(true)
    expect(awaitsElysia([ asked, later ])).toBe(true)
    expect(awaitsElysia([ asked, answered ])).toBe(false)
    expect(awaitsElysia([ asked, answered, comment('d', 't', 'ada', [ ELYSIA_USER_ID ]) ])).toBe(true)
    expect(awaitsElysia([ later ])).toBe(false)
  })
})

describe('mentionAt', () => {
  it('reads the name being typed after an @ that starts a word', () => {
    expect(mentionAt('Ask @Gra', 8)).toEqual({ start: 4, query: 'Gra' })
    expect(mentionAt('@', 1)).toEqual({ start: 0, query: '' })
    // The caret decides: what follows it is not typed yet.
    expect(mentionAt('Ask @Grace later', 8)).toEqual({ start: 4, query: 'Gra' })
  })

  it('reads no mention inside a word, past a space, or with none typed', () => {
    expect(mentionAt('ada@example.com', 15)).toBeNull()
    expect(mentionAt('Ask @Grace now', 14)).toBeNull()
    expect(mentionAt('No mention', 10)).toBeNull()
  })
})

describe('mentionCandidates', () => {
  const people = [
    user('grace', 'Grace Hopper'),
    user('ada', 'Ada Lovelace'),
    user('pending', 'Grant Waiting', { status: 'pending' }),
    user('system', 'System', { kind: 'machine', status: null, role: 'system' }),
    user(ELYSIA_USER_ID, 'Elysia', { kind: 'machine', status: null, role: 'agent' }),
  ]

  const everyone = () => true

  it('offers active people and Elysia, names that start with it before words that do', () => {
    expect(mentionCandidates(people, 'gr', everyone).map((candidate) => candidate.id)).toEqual([ 'grace' ])
    expect(mentionCandidates(people, 'love', everyone).map((candidate) => candidate.id)).toEqual([ 'ada' ])
    expect(mentionCandidates(people, 'ely', everyone).map((candidate) => candidate.id))
      .toEqual([ ELYSIA_USER_ID ])
    expect(mentionCandidates(people, '', everyone).map((candidate) => candidate.id))
      .toEqual([ 'grace', 'ada', ELYSIA_USER_ID ])
  })

  it('offers only the people who may read the meeting, and Elysia always', () => {
    const graceAlone = (candidate: { id: string }) => candidate.id === 'grace'
    expect(mentionCandidates(people, '', graceAlone).map((candidate) => candidate.id))
      .toEqual([ 'grace', ELYSIA_USER_ID ])
  })
})

describe('keptMentions', () => {
  it('keeps each one picked whose @Name is still written, once', () => {
    const names = { grace: 'Grace Hopper', ada: 'Ada Lovelace' }

    expect(keptMentions('@Grace Hopper and @Ada Lovelace', [ 'grace', 'ada', 'grace' ], names))
      .toEqual([ 'grace', 'ada' ])
    // A name deleted from the text, or someone unknown, is mentioned no more.
    expect(keptMentions('Just @Ada Lovelace', [ 'grace', 'ada', 'stranger' ], names)).toEqual([ 'ada' ])
  })

  it('keeps no one whose name is only the start of a longer name written', () => {
    const names = { ada: 'Ada', lovelace: 'Ada Lovelace' }

    expect(keptMentions('Ask @Ada Lovelace', [ 'ada', 'lovelace' ], names)).toEqual([ 'lovelace' ])
    expect(keptMentions('Ask @Ada and @Ada Lovelace', [ 'ada', 'lovelace' ], names))
      .toEqual([ 'ada', 'lovelace' ])
  })
})

describe('mentionPieces', () => {
  it('sets each mention apart, the longest name first', () => {
    expect(mentionPieces('Hi @Ada Lovelace, and @Ada', [ 'Ada', 'Ada Lovelace' ])).toEqual([
      { text: 'Hi ', isMention: false },
      { text: '@Ada Lovelace', isMention: true },
      { text: ', and ', isMention: false },
      { text: '@Ada', isMention: true },
    ])
    expect(mentionPieces('No one here', [])).toEqual([{ text: 'No one here', isMention: false }])
  })
})
