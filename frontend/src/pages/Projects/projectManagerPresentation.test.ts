// Copyright © 2026 Jalapeno Labs

import type { ProjectManagerWake, ProjectManagerWakeKind, WorkBatch } from '../../api/routes/projectManagerRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makePromise } from '../../testFixtures'
import { groupWakes, promiseWork } from './projectManagerPresentation'

function wake(id: string, kind: ProjectManagerWakeKind, body: string | null = null): ProjectManagerWake {
  return {
    id,
    kind,
    timelineRecordId: kind === 'record'
      ? `record-${id}`
      : null,
    workBatchId: kind === 'batch'
      ? `batch-${id}`
      : null,
    promiseId: kind === 'promise'
      ? `promise-${id}`
      : null,
    body,
    createdBy: 'someone',
    createdAt: '2026-11-04T15:00:00Z',
  }
}

describe('groupWakes', () => {
  it('keeps every message in order and counts the records, batches, and promises', () => {
    const grouped = groupWakes([
      wake('1', 'record'),
      wake('2', 'message', 'Ship it this week.'),
      wake('3', 'record'),
      wake('4', 'batch'),
      wake('5', 'message', 'And tell Ada.'),
      wake('6', 'promise'),
    ])

    expect(grouped.messages.map((message) => message.body)).toEqual([ 'Ship it this week.', 'And tell Ada.' ])
    expect(grouped.records).toBe(2)
    expect(grouped.batches).toBe(1)
    expect(grouped.promises).toBe(1)
  })

  it('reads a turn that nothing woke as empty', () => {
    expect(groupWakes([])).toEqual({ messages: [], records: 0, batches: 0, promises: 0 })
  })
})

describe('promiseWork', () => {
  const batch: WorkBatch = {
    id: 'batch-1',
    projectId: 'project-1',
    title: 'Fix the login bug',
    goal: 'Fix it, as Ada asked.',
    repositories: [],
    priority: 'asap',
    scheduledFor: null,
    state: 'running',
    result: null,
    turnId: null,
    sourceRecordIds: [],
    actionItemIds: [],
    timelineRecordId: null,
    createdAt: '2026-11-04T15:00:00Z',
    pinnedAt: null,
    assignedAt: null,
    startedAt: null,
    finishedAt: null,
    waitingOn: null,
    waitingDetail: null,
    waitingUntil: null,
  }

  it('names the batch a promise waits on by its title', () => {
    const work = promiseWork(makePromise({ id: 'promise-1', workBatchId: 'batch-1' }), [ batch ], {})
    expect(work).toEqual({ kind: 'batch', title: 'Fix the login bug', itemId: null })
  })

  it('names an item by its title and links to it, or by its kind when the page does not hold it', () => {
    const promise = makePromise({ id: 'promise-1', workItemId: 'item-1' })
    expect(promiseWork(promise, [], { 'item-1': 'Ship annual pricing' }))
      .toEqual({ kind: 'item', title: 'Ship annual pricing', itemId: 'item-1' })
    expect(promiseWork(promise, [], {})).toEqual({ kind: 'item', title: null, itemId: 'item-1' })
  })

  it('says when a promise waits on no work yet', () => {
    expect(promiseWork(makePromise({ id: 'promise-1' }), [], {})).toEqual({ kind: 'none', title: null, itemId: null })
  })
})
