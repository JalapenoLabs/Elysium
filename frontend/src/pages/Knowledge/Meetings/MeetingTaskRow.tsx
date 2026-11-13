// Copyright © 2026 Jalapeno Labs

import type { MeetingTask } from '../../../api/routes/meetingRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Chip, Link, Tooltip } from '@heroui/react'
import { LuTextSearch } from 'react-icons/lu'

// Misc
import { getActionItemViewUrl, getChangesetViewUrl } from '../../../urls'
import { stateChipColors, stateLabelKeys } from '../../ActionItems/actionItemPresentation'
import { taskStatus } from './meetingTasks'

type Props = {
  task: MeetingTask
  onShowWords: (startWordId: string, endWordId: string) => void
}

// One task in its group: what is to be done, a way to its words, and what came of it: the items
// made of it, each opening its page, the proposal waiting for a person, or why none was made.
export function MeetingTaskRow(props: Props) {
  const { t } = useTranslation([ 'meetings', 'actionItems' ])
  const { task } = props
  const status = taskStatus(task)
  const showWords = t('tasks.showWords')

  return <li className='text-sm'>
    <div className='flex items-start justify-between gap-2'>
      <span className='font-medium'>{task.title}</span>
      <Tooltip delay={300}>
        <Button
          isIconOnly
          size='sm'
          variant='ghost'
          aria-label={showWords}
          onPress={() => props.onShowWords(task.startWordId, task.endWordId)}
        >
          <LuTextSearch className='size-4' aria-hidden />
        </Button>
        <Tooltip.Content>
          <span>{showWords}</span>
        </Tooltip.Content>
      </Tooltip>
    </div>
    {status === 'made' && <ul className='mt-1 flex flex-col gap-1'>{
      task.items.map((item) => <li key={item.id} className='flex items-center gap-2'>
        <Link href={getActionItemViewUrl(item.id)} className='min-w-0 truncate'>{item.title}</Link>
        <Chip size='sm' variant='soft' color={stateChipColors[item.state]}>{
          t(`actionItems:${stateLabelKeys[item.state]}`)
        }</Chip>
      </li>)
    }</ul>}
    {status === 'proposed' && <Link href={getChangesetViewUrl(task.proposals[0])} className='mt-1'>{
      t('tasks.status.proposed')
    }</Link>}
    {status === 'declined' && <p className='mt-1 opacity-70'>{
      t('tasks.status.declined', { reason: task.declined?.reason })
    }</p>}
    {status === 'waiting' && <p className='mt-1 opacity-70'>{t('tasks.status.waiting')}</p>}
    {status === 'removed' && <p className='mt-1 opacity-70'>{t('tasks.status.removed')}</p>}
  </li>
}
