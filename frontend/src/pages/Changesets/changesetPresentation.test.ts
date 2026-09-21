// Copyright © 2026 Jalapeno Labs

import type { ChangesetOperation } from '../../api/routes/changesetRoutes'
import type { ChangesetTranslate, DescribeContext } from './changesetPresentation'

// Core
import i18next from 'i18next'
import { describe, expect, it } from 'vitest'

// Misc
import { makeActionItem, makeChangeset, makeChangesetOperation } from '../../testFixtures'
import { dependentsOf, describeOperation, describeUndo, tallyDecisions } from './changesetPresentation'

const t: ChangesetTranslate = i18next.getFixedT(null, [ 'changesets', 'actionItems' ])

const ITEM_ID = '01920000-0000-7000-8000-00000000000a'

function createItem(position: number, title: string): ChangesetOperation {
  return makeChangesetOperation({
    id: `create-${position}`,
    position,
    operation: {
      kind: 'create-item',
      title,
      notes: '',
      priority: 'normal',
      dueAt: null,
      projectIds: [],
      initiatives: [],
    },
  })
}

function commentOn(position: number, target: number): ChangesetOperation {
  return makeChangesetOperation({
    id: `comment-${position}`,
    position,
    operation: { kind: 'comment', item: { operation: target }, body: 'Sam will review it.' },
    dependsOn: [ target ],
  })
}

function context(operations: ChangesetOperation[]): DescribeContext {
  return {
    operations,
    itemsById: {
      [ITEM_ID]: makeActionItem({ id: ITEM_ID, title: 'Reply to Sam', priority: 'normal' }),
    },
    initiativeNames: {},
    projectNames: {},
    formatInstant: (instant) => instant,
  }
}

describe('dependentsOf', () => {
  it('finds everything downstream of an operation, through chains', () => {
    // 1 creates an item, 2 comments on it, 3 depends on 2, and 4 depends on nothing.
    const operations = [
      createItem(1, 'Draft'),
      commentOn(2, 1),
      makeChangesetOperation({
        id: 'third',
        position: 3,
        operation: { kind: 'resolve-item', item: { operation: 2 }},
        dependsOn: [ 2 ],
      }),
      makeChangesetOperation({
        id: 'fourth',
        position: 4,
        operation: { kind: 'resolve-item', item: { id: ITEM_ID }},
      }),
    ]
    expect(dependentsOf(operations, 1)).toEqual([ 2, 3 ])
    expect(dependentsOf(operations, 4)).toEqual([])
  })
})

describe('tallyDecisions', () => {
  it('allows applying only a pending changeset with every operation decided', () => {
    const undecided = makeChangeset({
      id: 'changeset',
      operations: [
        makeChangesetOperation({ ...createItem(1, 'Draft'), decision: 'approved' }),
        makeChangesetOperation({ ...commentOn(2, 1), decision: 'pending' }),
      ],
    })
    expect(tallyDecisions(undecided)).toEqual({ approved: 1, rejected: 0, pending: 1, canApply: false })

    const decided = makeChangeset({
      ...undecided,
      operations: undecided.operations.map((operation) => ({ ...operation, decision: 'rejected' as const })),
    })
    expect(tallyDecisions(decided).canApply).toBe(true)
    expect(tallyDecisions({ ...decided, state: 'rejected' }).canApply).toBe(false)
  })
})

describe('describeOperation', () => {
  it('names a record an earlier operation creates by its title and position', () => {
    const operations = [ createItem(1, 'Draft the launch post'), commentOn(2, 1) ]
    const description = describeOperation(operations[1], context(operations), t)
    expect(description.summary).toBe('Comment on Draft the launch post (from change 1)')
    expect(description.details).toEqual([ 'Sam will review it.' ])
  })

  it('shows an edit from the item as it is now, then from what applying replaced', () => {
    const update = makeChangesetOperation({
      id: 'update',
      position: 1,
      operation: { kind: 'update-item', item: { id: ITEM_ID }, title: 'Reply to Sam about logs', priority: 'high' },
    })
    const before = describeOperation(update, context([ update ]), t)
    expect(before.summary).toBe('Edit Reply to Sam')
    expect(before.details).toEqual([
      'Title: Reply to Sam → Reply to Sam about logs',
      'Priority: Normal → High',
    ])

    const applied = makeChangesetOperation({
      ...update,
      outcome: 'applied',
      result: { itemId: ITEM_ID, changes: { title: { from: 'Old title', to: 'Reply to Sam about logs' }}},
    })
    const after = describeOperation(applied, context([ applied ]), t)
    expect(after.details[0]).toBe('Title: Old title → Reply to Sam about logs')
  })

  it('says so when an item it names is gone', () => {
    const resolve = makeChangesetOperation({
      id: 'resolve',
      position: 1,
      operation: { kind: 'resolve-item', item: { id: 'gone' }},
    })
    expect(describeOperation(resolve, context([ resolve ]), t).summary).toBe('Resolve an item that is gone')
  })
})

describe('describeUndo', () => {
  it('names what the undo could not reverse', () => {
    const lines = describeUndo({
      kept: [ 'priority' ],
      stillPosted: { provider: 'jira', key: 'ELY-12' },
      stillClosed: [
        { provider: 'github', key: 'owner/name#1' },
        { provider: 'github', key: 'owner/name#2' },
      ],
    }, t)
    expect(lines).toEqual([
      'Kept your later change to Priority.',
      'Already posted to Jira ELY-12; it stays there.',
      'Already closed in GitHub: owner/name#1, owner/name#2. They stay closed.',
    ])
    expect(describeUndo({}, t)).toEqual([])
  })
})
