// Copyright © 2026 Jalapeno Labs

import type { ActionItem, ActionItemPriority } from '../api/routes/actionItemRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makeActionItem } from '../testFixtures'
import { isInNext, orderNext } from './nextOrder'

// The cases mirror api/src/action_items/next.rs, so the two orders are held to the same
// examples.
const NOW = Date.parse('2026-09-18T12:00:00Z')
const HOUR_MS = 3_600_000
const DAY_MS = 24 * HOUR_MS

function item(title: string, priority: ActionItemPriority, dueAt: number | null): ActionItem {
  return makeActionItem({
    id: title,
    title,
    priority,
    dueAt: dueAt === null
      ? null
      : new Date(dueAt).toISOString(),
  })
}

function titles(items: ActionItem[]) {
  return items.map((entry) => entry.title)
}

describe('orderNext', () => {
  it('leads with overdue items whatever their priority', () => {
    const items = [
      item('urgent, due later', 'urgent', NOW + DAY_MS),
      item('low, overdue', 'low', NOW - HOUR_MS),
      item('urgent, undated', 'urgent', null),
    ]

    expect(titles(orderNext(items, NOW))).toEqual([
      'low, overdue',
      'urgent, due later',
      'urgent, undated',
    ])
  })

  it('orders each group from urgent to low', () => {
    const items = [
      item('low', 'low', null),
      item('normal', 'normal', null),
      item('urgent', 'urgent', null),
      item('high', 'high', null),
    ]

    expect(titles(orderNext(items, NOW))).toEqual([ 'urgent', 'high', 'normal', 'low' ])
  })

  it('puts the most overdue first and undated items after dated ones', () => {
    const items = [
      item('undated', 'high', null),
      item('due tomorrow', 'high', NOW + DAY_MS),
      item('due next week', 'high', NOW + 7 * DAY_MS),
      item('a day late', 'high', NOW - DAY_MS),
      item('a week late', 'high', NOW - 7 * DAY_MS),
    ]

    expect(titles(orderNext(items, NOW))).toEqual([
      'a week late',
      'a day late',
      'due tomorrow',
      'due next week',
      'undated',
    ])
  })

  it('leads with older items among equals and treats an item due now as not yet overdue', () => {
    const newer = {
      ...item('newer', 'normal', NOW),
      createdAt: '2026-09-01T01:00:00.000Z',
    }
    const older = item('older', 'normal', NOW)
    const late = item('late', 'low', NOW - 1_000)

    expect(titles(orderNext([ newer, older, late ], NOW))).toEqual([ 'late', 'older', 'newer' ])
  })

  it('breaks a full tie by id', () => {
    const items = [
      makeActionItem({ id: 'b' }),
      makeActionItem({ id: 'a' }),
    ]

    expect(titles(orderNext(items, NOW))).toEqual([ 'a', 'b' ])
  })
})

describe('isInNext', () => {
  it('takes an open item the user owns', () => {
    expect(isInNext(makeActionItem({ id: 'open' }), NOW)).toBe(true)
  })

  it('leaves out every other state', () => {
    for (const state of [ 'inbox', 'resolved', 'dismissed' ] as const) {
      expect(isInNext(makeActionItem({ id: state, state }), NOW)).toBe(false)
    }
  })

  it('leaves out items owned by someone else or nobody', () => {
    const other = makeActionItem({ id: 'other', owner: { kind: 'other', name: 'Sam' }})
    const nobody = makeActionItem({ id: 'nobody', owner: { kind: 'nobody' }})

    expect(isInNext(other, NOW)).toBe(false)
    expect(isInNext(nobody, NOW)).toBe(false)
  })

  it('leaves out deleted items and items waiting on someone', () => {
    const deleted = makeActionItem({ id: 'deleted', deletedAt: '2026-09-02T00:00:00.000Z' })
    const waiting = makeActionItem({ id: 'waiting', waitingOn: 'sam@example.com' })

    expect(isInNext(deleted, NOW)).toBe(false)
    expect(isInNext(waiting, NOW)).toBe(false)
  })

  it('brings a snoozed item back the moment its snooze runs out', () => {
    const snoozed = makeActionItem({ id: 'snoozed', snoozedUntil: new Date(NOW).toISOString() })

    expect(isInNext(snoozed, NOW - 1)).toBe(false)
    expect(isInNext(snoozed, NOW)).toBe(true)
  })
})
