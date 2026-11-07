// Copyright © 2026 Jalapeno Labs

import type { BatchPullRequest, WorkQueueEntry } from '../../api/routes/workQueueRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { availableActions, currentWait, describeHistoryEntry, pullRequestStanding } from './workQueuePresentation'

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
    for (const state of [ 'running', 'succeeded', 'failed', 'cancelled', 'rejected' ] as const) {
      expect(availableActions(entry({ state }), NOW)).toEqual([])
    }
  })

  it('offers a proposal only a decision, never a place in the queue', () => {
    expect(availableActions(entry({ state: 'proposed' }), NOW)).toEqual([ 'approve', 'reject' ])
    const scheduled = entry({ state: 'proposed', priority: 'scheduled', scheduledFor: '2026-11-05T09:00:00Z' })
    expect(availableActions(scheduled, NOW)).toEqual([ 'approve', 'reject' ])
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
    expect(cancelled.note).toBe('Launch moved.')
    const finished = describeHistoryEntry({
      id: 'event-3',
      kind: 'finished',
      data: { succeeded: false },
      createdBy: 'system',
      createdAt: '2026-11-04T11:00:00Z',
    })
    expect(finished.succeeded).toBe(false)
  })

  it('names the fields a person changed before approving, priority and its time as one', () => {
    const edited = describeHistoryEntry({
      id: 'event-5',
      kind: 'edited',
      data: {
        before: { title: 'Pricing', priority: 'normal', scheduledFor: null },
        after: { title: 'Annual pricing', priority: 'scheduled', scheduledFor: '2026-11-05T09:00:00Z' },
      },
      createdBy: 'person-1',
      createdAt: '2026-11-04T11:00:00Z',
    })
    expect(edited.labelKey).toBe('history.kinds.edited')
    expect(edited.edited).toEqual([ 'title', 'priority' ])

    const unreadable = describeHistoryEntry({
      id: 'event-6',
      kind: 'edited',
      data: {},
      createdBy: 'person-1',
      createdAt: '2026-11-04T11:00:00Z',
    })
    expect(unreadable.edited).toEqual([])
  })

  it('carries why a person rejected a proposal', () => {
    const rejected = describeHistoryEntry({
      id: 'event-7',
      kind: 'rejected',
      data: { reason: 'Not this quarter.' },
      createdBy: 'person-1',
      createdAt: '2026-11-04T11:00:00Z',
    })
    expect(rejected.labelKey).toBe('history.kinds.rejected')
    expect(rejected.note).toBe('Not this quarter.')
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

  it('reads what a worker waits on, when it looks again, and what for', () => {
    const waiting = describeHistoryEntry({
      id: 'event-5',
      kind: 'waiting',
      data: { on: 'ci', detail: 'Waiting on the checks of #3.', until: '2026-11-04T13:00:00Z' },
      createdBy: 'coding-agent',
      createdAt: '2026-11-04T12:00:00Z',
    })
    expect(waiting.labelKey).toBe('history.waiting.ci')
    expect(waiting.until).toBe('2026-11-04T13:00:00Z')
    expect(waiting.note).toBe('Waiting on the checks of #3.')
  })

  it('still says a worker waits when it cannot read on what or until when', () => {
    for (const data of [
      { on: 'lunch', detail: 'Back soon.', until: '2026-11-04T13:00:00Z' },
      { on: 'review', detail: 'A review of #3.', until: 'tomorrow' },
      { on: 'review', detail: 'A review of #3.' },
    ]) {
      const waiting = describeHistoryEntry({
        id: 'event-6',
        kind: 'waiting',
        data,
        createdBy: 'coding-agent',
        createdAt: '2026-11-04T12:00:00Z',
      })
      expect(waiting.labelKey).toBe('history.kinds.waiting')
      expect(waiting.until).toBeNull()
      expect(waiting.note).toBe(data.detail)
    }
  })

  it('carries the answer its Project Manager gave', () => {
    const answered = describeHistoryEntry({
      id: 'event-7',
      kind: 'answered',
      data: { answer: 'Use the annual price.' },
      createdBy: 'project-managers',
      createdAt: '2026-11-04T12:30:00Z',
    })
    expect(answered.labelKey).toBe('history.kinds.answered')
    expect(answered.note).toBe('Use the annual price.')
    expect(answered.until).toBeNull()
  })
})

describe('currentWait', () => {
  const WAITING = {
    state: 'running',
    waitingOn: 'question',
    waitingDetail: 'Which plan should the banner name?',
    waitingUntil: '2026-11-04T18:00:00Z',
  } as const

  it('reads what a running batch waits on', () => {
    expect(currentWait(WAITING)).toEqual({
      on: 'question',
      detail: 'Which plan should the banner name?',
      until: '2026-11-04T18:00:00Z',
    })
  })

  it('reads no wait on a batch that is not running, or whose worker waits on nothing', () => {
    expect(currentWait({ ...WAITING, state: 'succeeded' })).toBeNull()
    expect(currentWait({ ...WAITING, waitingOn: null, waitingDetail: null, waitingUntil: null })).toBeNull()
  })
})

function pullRequest(overrides: Partial<BatchPullRequest> = {}): BatchPullRequest {
  return {
    id: 'pull-1',
    workBatchId: 'batch-1',
    repository: 'JalapenoLabs/web',
    number: 12,
    url: 'https://github.com/JalapenoLabs/web/pull/12',
    title: 'Fix the login page',
    state: 'open',
    mergedAt: null,
    mergedBy: null,
    mergedByElysium: false,
    mergeMethod: null,
    heldBecause: null,
    lookedAt: null,
    timelineRecordId: null,
    createdBy: '00000000-0000-0000-0000-000000000003',
    createdAt: '2026-11-04T10:00:00Z',
    ...overrides,
  }
}

describe('pullRequestStanding', () => {
  it('says Elysium merged it apart from a person merging it', () => {
    const byElysium = pullRequestStanding(pullRequest({
      state: 'merged',
      mergedBy: 'elysium-bot',
      mergedByElysium: true,
      mergeMethod: 'squash',
    }))
    expect(byElysium.labelKey).toBe('batch.pullRequests.mergedByElysium')
    expect(byElysium.color).toBe('success')

    const byPerson = pullRequestStanding(pullRequest({ state: 'merged', mergedBy: 'octocat' }))
    expect(byPerson.labelKey).toBe('batch.pullRequests.mergedBy')
    expect(byPerson.login).toBe('octocat')
  })

  it('keeps why an open one is held, and says a closed one never merged', () => {
    const held = pullRequestStanding(pullRequest({ heldBecause: '1 review thread is not resolved.' }))
    expect(held.labelKey).toBe('batch.pullRequests.open')
    expect(held.note).toBe('1 review thread is not resolved.')

    const closed = pullRequestStanding(pullRequest({ state: 'closed' }))
    expect(closed.labelKey).toBe('batch.pullRequests.closed')
    expect(closed.note).toBeNull()
  })
})
