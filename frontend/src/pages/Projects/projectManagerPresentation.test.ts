// Copyright © 2026 Jalapeno Labs

import type { ProjectManagerWake, ProjectManagerWakeKind } from '../../api/routes/projectManagerRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { groupWakes } from './projectManagerPresentation'

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
    body,
    createdBy: 'someone',
    createdAt: '2026-11-04T15:00:00Z',
  }
}

describe('groupWakes', () => {
  it('keeps every message in order and counts the records and batches', () => {
    const grouped = groupWakes([
      wake('1', 'record'),
      wake('2', 'message', 'Ship it this week.'),
      wake('3', 'record'),
      wake('4', 'batch'),
      wake('5', 'message', 'And tell Ada.'),
    ])

    expect(grouped.messages.map((message) => message.body)).toEqual([ 'Ship it this week.', 'And tell Ada.' ])
    expect(grouped.records).toBe(2)
    expect(grouped.batches).toBe(1)
  })

  it('reads a turn that nothing woke as empty', () => {
    expect(groupWakes([])).toEqual({ messages: [], records: 0, batches: 0 })
  })
})
