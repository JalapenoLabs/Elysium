// Copyright © 2026 Jalapeno Labs

import type { MeetingTask, MeetingTaskShare } from '../../../api/routes/meetingRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { groupTasks, taskStatus } from './meetingTasks'

function task(id: string, share: MeetingTaskShare, overrides: Partial<MeetingTask> = {}): MeetingTask {
  return {
    id,
    title: `Task ${id}`,
    startWordId: `${id}-start`,
    endWordId: `${id}-end`,
    startMs: 0,
    share,
    items: [],
    proposals: [],
    declined: null,
    ...overrides,
  }
}

describe('groupTasks', () => {
  it('groups each project first, then the person\'s own, each coworker, and Other', () => {
    const tasks = [
      task('own', { kind: 'own' }),
      task('nobody', { kind: 'other' }),
      task('launch-1', { kind: 'project', projectId: 'launch' }),
      task('grace-1', { kind: 'coworker', userId: 'grace', name: 'Grace Hopper' }),
      task('sharath', { kind: 'waiting-on', name: 'Sharath' }),
      task('budget', { kind: 'project', projectId: 'budget' }),
      task('grace-2', { kind: 'coworker', userId: 'grace', name: 'Grace Hopper' }),
      task('launch-2', { kind: 'project', projectId: 'launch' }),
      task('sharath-again', { kind: 'waiting-on', name: 'sharath' }),
    ]

    const groups = groupTasks(tasks)

    expect(groups.map((group) => [ group.key, group.tasks.map((grouped) => grouped.id) ])).toEqual([
      [ 'project:launch', [ 'launch-1', 'launch-2' ]],
      [ 'project:budget', [ 'budget' ]],
      [ 'own', [ 'own' ]],
      [ 'person:grace', [ 'grace-1', 'grace-2' ]],
      // Someone not on Elysium is one person however the transcript spells their name's case.
      [ 'name:sharath', [ 'sharath', 'sharath-again' ]],
      [ 'other', [ 'nobody' ]],
    ])
    expect(groups[3]).toMatchObject({ kind: 'coworker', name: 'Grace Hopper', isOnElysium: true })
    expect(groups[4]).toMatchObject({ kind: 'coworker', name: 'Sharath', isOnElysium: false })
  })

  it('leaves out every group with no task', () => {
    expect(groupTasks([])).toEqual([])
    expect(groupTasks([ task('own', { kind: 'own' }) ]).map((group) => group.key)).toEqual([ 'own' ])
  })
})

describe('taskStatus', () => {
  const project: MeetingTaskShare = { kind: 'project', projectId: 'launch' }

  it('reads an item made before a proposal or a reason', () => {
    const made = task('made', project, {
      items: [{ id: 'item', title: 'Fix it', state: 'open' }],
      proposals: [ 'changeset' ],
      declined: { reason: 'Covered.', at: '2026-11-12T10:00:00Z' },
    })
    expect(taskStatus(made)).toBe('made')
    expect(taskStatus(task('proposed', project, { proposals: [ 'changeset' ]}))).toBe('proposed')
    expect(taskStatus(task('declined', project, {
      declined: { reason: 'Covered.', at: '2026-11-12T10:00:00Z' },
    }))).toBe('declined')
  })

  it('waits on a project\'s Project Manager, and reads a task on no project with no item as removed', () => {
    expect(taskStatus(task('waiting', project))).toBe('waiting')
    expect(taskStatus(task('own', { kind: 'own' }))).toBe('removed')
  })
})
