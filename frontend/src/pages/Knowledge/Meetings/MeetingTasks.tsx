// Copyright © 2026 Jalapeno Labs

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { MeetingTaskGroup } from './MeetingTaskGroup'

// Misc
import { groupTasks } from './meetingTasks'
import { useMeetingTasks } from './useMeetingTasks'

type Props = {
  meetingId: string
  // Selects a task's words on the transcript and brings them into view.
  onShowWords: (startWordId: string, endWordId: string) => void
}

// The bar along the top of a meeting's page that counts its tasks (docs/meeting-review.md, Tasks):
// each project's, the person's own, each coworker's, and Other, each opening its tasks with the
// items made of them. A meeting in which nobody took anything on shows nothing.
export function MeetingTasks(props: Props) {
  const { t } = useTranslation('meetings')
  const tasks = useMeetingTasks(props.meetingId)
  const groups = useMemo(() => groupTasks(tasks ?? []), [ tasks ])
  if (!groups.length) {
    return null
  }

  return <section aria-labelledby='meeting-tasks-heading' className='relaxed flex flex-wrap items-center gap-2'>
    <h2 id='meeting-tasks-heading' className='text-sm font-semibold'>{t('tasks.heading')}</h2>
    {groups.map((group) => <MeetingTaskGroup
      key={group.key}
      group={group}
      onShowWords={props.onShowWords}
    />)}
  </section>
}
