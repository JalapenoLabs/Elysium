// Copyright © 2026 Jalapeno Labs

import type { MeetingHistoryEntry } from '../../../api/routes/meetingRoutes'
import type { User } from '../../../api/routes/userRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makeMeeting } from '../../../testFixtures'
import { correctionDecisionOf, mayRead, mayUndo } from './meetingVisibility'

const OWNER = 'owner'

function entry(data: Record<string, unknown>): MeetingHistoryEntry {
  return {
    id: 'entry',
    kind: 'visibility_changed',
    createdBy: 'system',
    agent: null,
    data,
    undid: null,
    createdAt: '2026-11-11T09:00:00Z',
  }
}

function person(id: string, role: User['role']): User {
  return {
    id,
    kind: 'person',
    name: id,
    email: null,
    role,
    status: 'active',
    approvedAt: null,
    lastSeenAt: null,
    createdAt: '2026-11-11T09:00:00Z',
    updatedAt: '2026-11-11T09:00:00Z',
  }
}

describe('correctionDecisionOf', () => {
  it('says what the correction decided while its decision is the newest change', () => {
    expect(correctionDecisionOf(entry({ correction: true, after: 'public' }))?.key)
      .toBe('visibility.decisions.opened')
    expect(correctionDecisionOf(entry({ correction: true, after: 'private', undecided: true }))?.key)
      .toBe('visibility.decisions.undecided')
    expect(correctionDecisionOf(entry({ correction: true, after: 'private', why: '  A review. ' })))
      .toEqual({ key: 'visibility.decisions.keptPrivateBecause', why: 'A review.' })
    expect(correctionDecisionOf(entry({ correction: true, after: 'private', why: null }))?.key)
      .toBe('visibility.decisions.keptPrivate')
  })

  it('says nothing once a person changed who reads it, or before anyone decided', () => {
    expect(correctionDecisionOf(entry({ before: 'unchecked', after: 'public' }))).toBeNull()
    expect(correctionDecisionOf(null)).toBeNull()
  })
})

describe('mayRead', () => {
  it('lets its owner, whom it is shared with, and admins read a private meeting', () => {
    const meeting = makeMeeting({ id: 'm', ownerId: OWNER, visibility: 'private', sharedWith: [ 'shared' ]})
    expect(mayRead(meeting, person(OWNER, 'member'))).toBe(true)
    expect(mayRead(meeting, person('shared', 'guest'))).toBe(true)
    expect(mayRead(meeting, person('admin', 'admin'))).toBe(true)
    expect(mayRead(meeting, person('member', 'member'))).toBe(false)
  })

  it('lets every member read a public meeting, and a guest only one shared with them', () => {
    const meeting = makeMeeting({ id: 'm', ownerId: OWNER, visibility: 'public' })
    expect(mayRead(meeting, person('member', 'member'))).toBe(true)
    expect(mayRead(meeting, person('guest', 'guest'))).toBe(false)
  })

  it('reads a meeting still unchecked as private', () => {
    const meeting = makeMeeting({ id: 'm', ownerId: OWNER, visibility: 'unchecked' })
    expect(mayRead(meeting, person('member', 'member'))).toBe(false)
  })
})

describe('mayUndo', () => {
  it('lets an editor undo edits, a manager undo who reads it, and the owner alone their processing', () => {
    const asMember = makeMeeting({ id: 'm', ownerId: OWNER, canEdit: true, canManage: false })
    expect(mayUndo(asMember, 'member', 'edited')).toBe(true)
    expect(mayUndo(asMember, 'member', 'visibility_changed')).toBe(false)
    expect(mayUndo(asMember, 'member', 'shared')).toBe(false)
    expect(mayUndo(asMember, 'member', 'hold_changed')).toBe(false)
    expect(mayUndo(asMember, OWNER, 'hold_changed')).toBe(true)

    const asGuest = makeMeeting({ id: 'm', ownerId: OWNER, canEdit: false, canManage: false })
    expect(mayUndo(asGuest, 'guest', 'edited')).toBe(false)
  })
})
