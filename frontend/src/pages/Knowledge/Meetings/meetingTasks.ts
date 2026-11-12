// Copyright © 2026 Jalapeno Labs

import type { MeetingTask } from '../../../api/routes/meetingRoutes'

// The tasks heard in a meeting as the bar along the top of its page counts them
// (docs/meeting-review.md, Tasks): each project's, the person's own, each coworker's, and Other.
// Pure, so the grouping is tested alone.

// One button of the bar: whose tasks they are, and the tasks, in the order they were said.
export type TaskGroup =
  | { key: string, kind: 'project', projectId: string, tasks: MeetingTask[] }
  | { key: string, kind: 'own', tasks: MeetingTask[] }
  // A coworker on Elysium by their id, or someone the person waits on by the name heard.
  | { key: string, kind: 'coworker', name: string, isOnElysium: boolean, tasks: MeetingTask[] }
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
  // A task on no project whose item was deleted since.
  | 'removed'

// Every task in one pass, grouped in the bar's order: the projects in the order their first task
// was said, the person's own, the coworkers in the same order, then Other. A coworker on Elysium is
// one group by their id, and someone not on it one by their name, as heard.
export function groupTasks(tasks: MeetingTask[]): TaskGroup[] {
  const projects = new Map<string, TaskGroup>()
  const coworkers = new Map<string, TaskGroup>()
  const own: TaskGroup = { key: 'own', kind: 'own', tasks: []}
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
    if (share.kind === 'coworker' || share.kind === 'waiting-on') {
      const isOnElysium = share.kind === 'coworker'
      const key = isOnElysium
        ? `person:${share.userId}`
        : `name:${share.name.toLowerCase()}`
      const group = coworkers.get(key) ?? { key, kind: 'coworker', name: share.name, isOnElysium, tasks: []}
      group.tasks.push(task)
      coworkers.set(key, group)
      continue
    }
    if (share.kind === 'own') {
      own.tasks.push(task)
      continue
    }
    other.tasks.push(task)
  }

  return [
    ...projects.values(),
    own,
    ...coworkers.values(),
    other,
  ].filter((group) => group.tasks.length)
}

// Where `task`'s item stands: made, proposed, or declined, in that order, since an item made
// answers the task whatever else was said of it.
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
  if (task.share.kind === 'project') {
    return 'waiting'
  }
  return 'removed'
}
