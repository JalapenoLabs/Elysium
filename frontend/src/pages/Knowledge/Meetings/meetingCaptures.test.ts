// Copyright © 2026 Jalapeno Labs

import type { MeetingCapture } from '../../../api/routes/meetingRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { groupCaptures, offsetSourceOf, startOf } from './meetingCaptures'

function capture(id: string, overrides: Partial<MeetingCapture> = {}): MeetingCapture {
  return {
    id,
    connectorId: null,
    sourceKind: 'fireflies',
    url: null,
    offsetMs: 0,
    offsetSource: 'listed',
    hasRecording: true,
    mergedFrom: null,
    ...overrides,
  }
}

const teamsReview = { meetingId: 'teams-meeting', title: 'Launch review' }
const huddle = { meetingId: 'huddle-meeting', title: '' }

describe('groupCaptures', () => {
  it('lists the meeting\'s own captures first, then each meeting merged in, in the order they joined', () => {
    const groups = groupCaptures([
      capture('own'),
      capture('teams-audio', { mergedFrom: teamsReview, offsetSource: 'audio' }),
      capture('huddle', { mergedFrom: huddle }),
      capture('teams-words', { mergedFrom: teamsReview, offsetSource: 'words' }),
    ])

    expect(groups.map((group) => [ group.from?.meetingId ?? null, group.captures.map((each) => each.id) ])).toEqual([
      [ null, [ 'own' ]],
      [ 'teams-meeting', [ 'teams-audio', 'teams-words' ]],
      [ 'huddle-meeting', [ 'huddle' ]],
    ])
    expect(groups.map((group) => group.canSplit)).toEqual([ false, true, true ])
  })

  it('holds a split back while a capture the merge brought was moved by hand since', () => {
    const [ moved ] = groupCaptures([
      capture('found', { mergedFrom: teamsReview, offsetSource: 'audio' }),
      capture('nudged', { mergedFrom: teamsReview, offsetSource: 'hand' }),
    ])

    expect(moved.from).toEqual(teamsReview)
    expect(moved.canSplit).toBe(false)
  })

  it('answers no group for a meeting with no captures of its own, such as one merged away', () => {
    expect(groupCaptures([])).toEqual([])
    const [ only ] = groupCaptures([ capture('brought', { mergedFrom: huddle }) ])
    expect(only.from).toEqual(huddle)
  })
})

describe('startOf', () => {
  it('says whether a capture starts with the meeting, into it, or before it, by how much', () => {
    expect(startOf(0)).toEqual({ key: 'review.captures.startsWith', spanMs: 0 })
    expect(startOf(2_500)).toEqual({ key: 'review.captures.startsInto', spanMs: 2_500 })
    expect(startOf(-1_500)).toEqual({ key: 'review.captures.startsBefore', spanMs: 1_500 })
  })
})

describe('offsetSourceOf', () => {
  it('reads how an offset was found, and nothing for a way this build does not know', () => {
    expect(offsetSourceOf(capture('nudged', { offsetSource: 'hand' }))).toBe('hand')
    expect(offsetSourceOf(capture('later', { offsetSource: 'video' }))).toBeNull()
  })
})
