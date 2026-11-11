// Copyright © 2026 Jalapeno Labs

import type { MeetingComment, MeetingMark } from '../../../api/routes/meetingRoutes'
import type { User } from '../../../api/routes/userRoutes'
import type { PlacedMark } from './meetingTranscript'

// Misc
import { ELYSIA_USER_ID } from '../../../constants'

// How a meeting's comment threads read on its page (docs/meeting-review.md, Comments), and how a
// comment names whom it mentions. Pure, so the components only draw what these answer.

// A thread as the page shows it: its mark where it lies, whether it is resolved, and its comments,
// oldest first.
export type Thread = {
  placed: PlacedMark
  isResolved: boolean
  comments: MeetingComment[]
}

// The most people the mention list offers at once; typing more of a name narrows it.
const MENTION_CANDIDATES_MAX = 6

// How long a name may be typed after `@` before it reads as plain text rather than a mention.
const MENTION_QUERY_MAX_CHARACTERS = 40

export function isResolvedThread(mark: MeetingMark) {
  return mark.data.resolved === true
}

// Every thread over the transcript as it reads, in the order their ranges start, open ones and
// resolved ones apart, in one pass over the marks.
export function threadsOf(placed: PlacedMark[], comments: MeetingComment[]) {
  const commentsByThread = new Map<string, MeetingComment[]>()
  for (const comment of comments) {
    const thread = commentsByThread.get(comment.threadId)
    if (thread) {
      thread.push(comment)
      continue
    }
    commentsByThread.set(comment.threadId, [ comment ])
  }

  const open: Thread[] = []
  const resolved: Thread[] = []
  for (const placedMark of placed) {
    if (placedMark.kind !== 'comment') {
      continue
    }
    const thread: Thread = {
      placed: placedMark,
      isResolved: isResolvedThread(placedMark.mark),
      comments: commentsByThread.get(placedMark.mark.id) ?? [],
    }
    if (thread.isResolved) {
      resolved.push(thread)
      continue
    }
    open.push(thread)
  }
  return { open, resolved } as const
}

// Whether Elysia was asked in the thread and has not answered since: its newest comment that
// mentions her has none of hers after it.
export function awaitsElysia(comments: MeetingComment[]) {
  for (let index = comments.length - 1; index >= 0; index -= 1) {
    const comment = comments[index]
    if (comment.createdBy === ELYSIA_USER_ID) {
      return false
    }
    if (comment.mentions.includes(ELYSIA_USER_ID)) {
      return true
    }
  }
  return false
}

export type MentionQuery = {
  // Where its `@` is in the text.
  start: number
  // What was typed after the `@`, up to the caret.
  query: string
}

// The mention being typed at `caret`: an `@` that starts a word, and what follows it up to the
// caret, when that is no more than a name's length with no space.
export function mentionAt(text: string, caret: number): MentionQuery | null {
  const before = text.slice(0, caret)
  const at = before.lastIndexOf('@')
  if (at === -1) {
    return null
  }
  const startsWord = at === 0 || /\s/.test(before[at - 1])
  const query = before.slice(at + 1)
  if (!startsWord || /[\s@]/.test(query) || query.length > MENTION_QUERY_MAX_CHARACTERS) {
    return null
  }
  return { start: at, query }
}

// Who `query` may mention, best match first: active people and Elysia, whose name starts with it
// or has a word that does, at most `MENTION_CANDIDATES_MAX`.
export function mentionCandidates(users: User[], query: string) {
  const wanted = query.toLowerCase()
  const startsName: User[] = []
  const startsWord: User[] = []
  for (const user of users) {
    const isMentionable = user.id === ELYSIA_USER_ID
      || (user.kind === 'person' && user.status === 'active')
    if (!isMentionable) {
      continue
    }
    const name = user.name.toLowerCase()
    if (name.startsWith(wanted)) {
      startsName.push(user)
      continue
    }
    if (name.split(/\s+/).some((part) => part.startsWith(wanted))) {
      startsWord.push(user)
    }
  }
  return [ ...startsName, ...startsWord ].slice(0, MENTION_CANDIDATES_MAX)
}

// The mentions a comment keeps: each one chosen whose `@Name` is still in its text, once each, in
// the order chosen. One deleted from the text is not mentioned, and neither is one whose name is
// only the start of a longer name chosen too (`@Ada` inside `@Ada Lovelace`).
export function keptMentions(body: string, chosen: string[], namesById: Record<string, string>) {
  const names = chosen.map((userId) => namesById[userId] ?? '')
  const written = new Set<string>()
  for (const piece of mentionPieces(body, names)) {
    if (piece.isMention) {
      written.add(piece.text)
    }
  }

  const kept: string[] = []
  for (const userId of chosen) {
    const name = namesById[userId]
    if (!name || kept.includes(userId) || !written.has(`@${name}`)) {
      continue
    }
    kept.push(userId)
  }
  return kept
}

export type CommentPiece = {
  text: string
  isMention: boolean
}

// A comment's text in pieces, each `@Name` of someone it mentions apart, so it draws as a
// mention. Longer names are matched first, so `@Ada Lovelace` is not read as `@Ada`.
export function mentionPieces(body: string, mentionedNames: string[]): CommentPiece[] {
  const handles = mentionedNames
    .filter(Boolean)
    .map((name) => `@${name}`)
    .toSorted((first, second) => second.length - first.length)
  const pieces: CommentPiece[] = []
  let plain = ''
  let index = 0
  while (index < body.length) {
    const handle = handles.find((candidate) => body.startsWith(candidate, index))
    if (!handle) {
      plain += body[index]
      index += 1
      continue
    }
    if (plain) {
      pieces.push({ text: plain, isMention: false })
      plain = ''
    }
    pieces.push({ text: handle, isMention: true })
    index += handle.length
  }
  if (plain) {
    pieces.push({ text: plain, isMention: false })
  }
  return pieces
}
