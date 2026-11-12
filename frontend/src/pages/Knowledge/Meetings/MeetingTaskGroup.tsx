// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'
import type { TaskGroup } from './meetingTasks'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectProjectEntities } from '../../../store/projectsSlice'

// User interface
import { Button, Chip, Popover } from '@heroui/react'
import { MeetingTaskRow } from './MeetingTaskRow'

// Misc
import { formatNumber } from '../../../format/numbers'

type Props = {
  group: TaskGroup
  onShowWords: (startWordId: string, endWordId: string) => void
}

type ProjectEntities = ReturnType<typeof selectProjectEntities>

// One button of a meeting's task bar: whose tasks they are and how many, opening each with what
// came of it.
export function MeetingTaskGroup(props: Props) {
  const { t } = useTranslation('meetings')
  const projects = useAppSelector(selectProjectEntities)
  const { group } = props
  const count = group.tasks.length
  const name = nameOf(group, projects, t)

  return <Popover>
    <Button
      size='sm'
      variant='secondary'
      aria-label={t('tasks.groupLabel', { name, count, formattedCount: formatNumber(count) })}
    >
      <span>{name}</span>
      <Chip size='sm' variant='soft' className='tabular-nums'>{formatNumber(count)}</Chip>
    </Button>
    <Popover.Content placement='bottom start' className='w-80 max-w-[calc(100vw-2rem)]'>
      <Popover.Dialog>
        <h3 className='compact font-semibold'>{name}</h3>
        <ul className='flex max-h-96 flex-col gap-3 overflow-y-auto'>{
          group.tasks.map((task) => <MeetingTaskRow
            key={task.id}
            task={task}
            onShowWords={props.onShowWords}
          />)
        }</ul>
      </Popover.Dialog>
    </Popover.Content>
  </Popover>
}

// What a group is called: its project, the person's own, a coworker by name, or someone the person
// waits on, who is not on Elysium.
function nameOf(group: TaskGroup, projects: ProjectEntities, t: TFunction<'meetings'>) {
  if (group.kind === 'project') {
    return projects[group.projectId]?.name ?? t('tasks.unknownProject')
  }
  if (group.kind === 'coworker') {
    if (group.isOnElysium) {
      return group.name
    }
    return t('tasks.waitingOn', { name: group.name })
  }
  if (group.kind === 'own') {
    return t('tasks.own')
  }
  return t('tasks.other')
}
