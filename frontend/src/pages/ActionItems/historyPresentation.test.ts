// Copyright © 2026 Jalapeno Labs

import type { HistoryContext, HistoryTranslate } from './historyPresentation'

// Core
import { beforeEach, describe, expect, it } from 'vitest'
import i18next from 'i18next'

// Misc
import { makeHistoryEntry } from '../../testFixtures'
import { describeHistoryEntry } from './historyPresentation'

type Context = {
  t: HistoryTranslate
  context: HistoryContext
}

describe('describeHistoryEntry', () => {
  beforeEach<Context>((testContext) => {
    // The app's own en-US strings, loaded by the test setup.
    testContext.t = i18next.getFixedT(null, [ 'actionItems', 'initiatives' ])
    testContext.context = {
      subject: 'item',
      projectNames: { p1: 'Elysium' },
      initiativeNames: { i1: 'Ship storage' },
      itemTitles: { a1: 'Reply to Sam' },
      formatInstant: (instant) => `<${instant}>`,
    }
  })

  it<Context>('lists each field an edit changed, from and to', ({ t, context }) => {
    const entry = makeHistoryEntry({
      id: 'e1',
      kind: 'updated',
      data: {
        changes: {
          priority: { from: 'normal', to: 'urgent' },
          dueAt: { from: null, to: '2026-09-20T03:59:59.999Z' },
          notes: { from: 'a', to: 'b' },
        },
      },
    })

    expect(describeHistoryEntry(entry, context, t)).toEqual({
      summary: 'edited',
      details: [
        'Priority: Normal to Urgent',
        'Due date: none to <2026-09-20T03:59:59.999Z>',
        'Notes changed',
      ],
    })
  })

  it<Context>('names states in words', ({ t, context }) => {
    const entry = makeHistoryEntry({ id: 'e1', kind: 'state_changed', data: { from: 'inbox', to: 'open' }})

    expect(describeHistoryEntry(entry, context, t).summary).toBe('moved it from Inbox to Open')
  })

  it<Context>('names the project, or says it was deleted', ({ t, context }) => {
    const known = makeHistoryEntry({ id: 'e1', kind: 'project_added', data: { projectId: 'p1' }})
    const gone = makeHistoryEntry({ id: 'e2', kind: 'project_removed', data: { projectId: 'p9' }})

    expect(describeHistoryEntry(known, context, t).summary).toBe('added it to Elysium')
    expect(describeHistoryEntry(gone, context, t).summary).toBe('removed it from a deleted project')
  })

  it<Context>('names the other side of a membership change for each history', ({ t, context }) => {
    const entry = makeHistoryEntry({
      id: 'e1',
      kind: 'initiative_joined',
      actionItemId: 'a1',
      initiativeId: 'i1',
    })

    expect(describeHistoryEntry(entry, context, t).summary).toBe('added it to the initiative Ship storage')
    expect(describeHistoryEntry(entry, { ...context, subject: 'initiative' }, t).summary).toBe('added Reply to Sam')
  })

  it<Context>('quotes a comment and keeps a deleted one readable', ({ t, context }) => {
    const entry = makeHistoryEntry({ id: 'e1', kind: 'comment_deleted', data: { commentId: 'c1', body: 'Looks good' }})

    expect(describeHistoryEntry(entry, context, t)).toEqual({
      summary: 'deleted a comment',
      details: [ 'Looks good' ],
    })
  })

  it<Context>('still says something for a kind or a shape it does not know', ({ t, context }) => {
    const future = makeHistoryEntry({ id: 'e1', kind: 'linked' })
    const malformed = makeHistoryEntry({ id: 'e2', kind: 'state_changed', data: null })

    expect(describeHistoryEntry(future, context, t).summary).toBe('recorded linked')
    expect(describeHistoryEntry(malformed, context, t).summary).toBe('changed its state')
  })
})
