// Copyright © 2026 Jalapeno Labs

import type { MeetingTask } from '../../../api/routes/meetingRoutes'

// The tasks heard in a meeting as the bar along the top of its page counts them
// (docs/meeting-review.md, Tasks): each project's, the person's own, each coworker's, and Other.
// Pure, so the grouping is tested alone.

// One button of the bar: whose tasks they are, and the tasks, in the order they were said.
export type TaskGroup =
  | { key: string, kind: 'project', projectId: string, tasks: MeetingTask[] }
  // The meeting's person's own, or a coworker's on Elysium, by their id, which reads Yours for them.
  | { key: string, kind: 'person', userId: string, name: string, tasks: MeetingTask[] }
  // Someone the person waits on, who is not on Elysium, by the name heard.
  | { key: string, kind: 'waiting-on', name: string, tasks: MeetingTask[] }
  | { key: string, kind: 'other', tasks: MeetingTask[] }

// Where a task's item stands, as its row in the bar says.
export type TaskStatus =
  // Made into one or more items, which open from the row.
  | 'made'
  // Its Project Manager proposed an item, waiting for a person's approval.
  | 'proposed'
  // Its Project Manager said why it made none.
  | 'declined'
  // A project's task its Project Manager has not answered yet.
  | 'waiting'
  // A task whose items were all deleted since.
  | 'removed'

// Every task in one pass, grouped in the bar's order: the projects in the order their first task
// was said, the person's own, the coworkers in the same order, then Other. A coworker on Elysium is
// one group by their id, and someone not on it one by their name, as heard.
export function groupTasks(tasks: MeetingTask[]): TaskGroup[] {
  const projects = new Map<string, TaskGroup>()
  const coworkers = new Map<string, TaskGroup>()
  let own: TaskGroup | null = null
  const other: TaskGroup = { key: 'other', kind: 'other', tasks: []}
  for (const task of tasks) {
    const share = task.share
    if (share.kind === 'project') {
      const key = `project:${share.projectId}`
      const group = projects.get(key) ?? { key, kind: 'project', projectId: share.projectId, tasks: []}
      group.tasks.push(task)
      projects.set(key, group)
      continue
    }
    if (share.kind === 'own') {
      own ??= { key: `person:${share.userId}`, kind: 'person', userId: share.userId, name: share.name, tasks: []}
      own.tasks.push(task)
      continue
    }
    if (share.kind === 'coworker') {
      const key = `person:${share.userId}`
      const group = coworkers.get(key)
        ?? { key, kind: 'person', userId: share.userId, name: share.name, tasks: []}
      group.tasks.push(task)
      coworkers.set(key, group)
      continue
    }
    if (share.kind === 'waiting-on') {
      const key = `name:${share.name.toLowerCase()}`
      const group = coworkers.get(key) ?? { key, kind: 'waiting-on', name: share.name, tasks: []}
      group.tasks.push(task)
      coworkers.set(key, group)
      continue
    }
    other.tasks.push(task)
  }

  const groups: TaskGroup[] = [ ...projects.values() ]
  if (own) {
    groups.push(own)
  }
  groups.push(...coworkers.values())
  if (other.tasks.length) {
    groups.push(other)
  }
  return groups
}

// Where `task`'s item stands: made, proposed, or declined, in that order, since an item made
// answers the task whatever else was said of it. One whose items were all deleted waits for
// nothing, since nobody is told to make it again.
export function taskStatus(task: MeetingTask): TaskStatus {
  if (task.items.length) {
    return 'made'
  }
  if (task.proposals.length) {
    return 'proposed'
  }
  if (task.declined) {
    return 'declined'
  }
  if (task.share.kind === 'project' && !task.deletedItems) {
    return 'waiting'
  }
  return 'removed'
}
