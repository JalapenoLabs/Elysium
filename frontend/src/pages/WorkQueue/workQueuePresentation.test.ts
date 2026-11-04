// Copyright © 2026 Jalapeno Labs

import type { WorkQueueEntry } from '../../api/routes/workQueueRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { availableActions, describeHistoryEntry } from './workQueuePresentation'

const NOW = Date.parse('2026-11-04T12:00:00Z')

function entry(overrides: Partial<WorkQueueEntry> = {}): WorkQueueEntry {
  return {
    id: 'batch-1',
    projectId: 'project-1',
    title: 'Annual pricing',
    priority: 'normal',
    scheduledFor: null,
    state: 'queued',
    pinnedAt: null,
    assignedAt: null,
    createdAt: '2026-11-04T10:00:00Z',
    startedAt: null,
    ...overrides,
  }
}

describe('availableActions', () => {
  it('offers a queued batch every change', () => {
    expect(availableActions(entry(), NOW)).toEqual([ 'runNext', 'makeAsap', 'pause', 'cancel' ])
  })

  it('offers a batch asked to run next its place back', () => {
    const pinned = entry({ pinnedAt: '2026-11-04T11:00:00Z' })
    expect(availableActions(pinned, NOW)).toEqual([ 'returnToPlace', 'makeAsap', 'pause', 'cancel' ])
  })

  it('offers an ASAP batch normal priority instead', () => {
    expect(availableActions(entry({ priority: 'asap' }), NOW)).toContain('makeNormal')
    expect(availableActions(entry({ priority: 'asap' }), NOW)).not.toContain('makeAsap')
  })

  it('never runs a batch scheduled for later next, but runs it as soon as possible', () => {
    const later = entry({ priority: 'scheduled', scheduledFor: '2026-11-05T09:00:00Z' })
    expect(availableActions(later, NOW)).toEqual([ 'makeAsap', 'pause', 'cancel' ])

    const due = entry({ priority: 'scheduled', scheduledFor: '2026-11-04T09:00:00Z' })
    expect(availableActions(due, NOW)).toContain('runNext')
  })

  it('offers a paused batch to resume, and one in a slot no worker started on to pause', () => {
    expect(availableActions(entry({ state: 'paused' }), NOW)).toEqual([ 'resume', 'cancel' ])
    expect(availableActions(entry({ state: 'assigned' }), NOW)).toEqual([ 'pause', 'cancel' ])
  })

  it('offers nothing once a worker started on it or it ended', () => {
    for (const state of [ 'running', 'succeeded', 'failed', 'cancelled' ] as const) {
      expect(availableActions(entry({ state }), NOW)).toEqual([])
    }
  })
})

describe('describeHistoryEntry', () => {
  it('reads a change of priority from what it was to what it is', () => {
    const described = describeHistoryEntry({
      id: 'event-1',
      kind: 'reprioritized',
      data: {
        before: { priority: 'normal', scheduledFor: null },
        after: { priority: 'scheduled', scheduledFor: '2026-11-05T09:00:00Z' },
      },
      createdBy: 'person-1',
      createdAt: '2026-11-04T11:00:00Z',
    })
    expect(described.labelKey).toBe('history.kinds.reprioritized')
    expect(described.before).toEqual({ priority: 'normal', scheduledFor: null })
    expect(described.after).toEqual({ priority: 'scheduled', scheduledFor: '2026-11-05T09:00:00Z' })
  })

  it('carries why a batch was cancelled and whether it succeeded', () => {
    const cancelled = describeHistoryEntry({
      id: 'event-2',
      kind: 'cancelled',
      data: { reason: 'Launch moved.' },
      createdBy: 'person-1',
      createdAt: '2026-11-04T11:00:00Z',
    })
    expect(cancelled.reason).toBe('Launch moved.')
    const finished = describeHistoryEntry({
      id: 'event-3',
      kind: 'finished',
      data: { succeeded: false },
      createdBy: 'system',
      createdAt: '2026-11-04T11:00:00Z',
    })
    expect(finished.succeeded).toBe(false)
  })

  it('keeps an entry of a kind this build does not know, without words for it', () => {
    const unknown = describeHistoryEntry({
      id: 'event-4',
      kind: 'teleported',
      data: { before: 'somewhere' },
      createdBy: 'system',
      createdAt: '2026-11-04T11:00:00Z',
    })
    expect(unknown.labelKey).toBeNull()
    expect(unknown.before).toBeNull()
  })
})
