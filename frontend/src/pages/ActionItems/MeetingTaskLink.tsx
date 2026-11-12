// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { buttonVariants, Link } from '@heroui/react'
import { LuTextSearch } from 'react-icons/lu'

// Misc
import { getTaskOfItem } from '../../api/routes/meetingRoutes'
import { getMeetingMarkUrl } from '../../urls'

type Props = {
  itemId: string
}

// The task heard in a meeting an item was made of, linked to the meeting's page opened at its
// words. An item no task made, or one about a meeting of someone else's, shows nothing: a meeting
// is its person's alone.
export function MeetingTaskLink(props: Props) {
  const { t } = useTranslation('meetings')
  const { data } = useSWR(
    [ 'v1/meetings/task-items', props.itemId ],
    () => getTaskOfItem(props.itemId),
    {
      onError: (error: unknown) => console.debug('MeetingTaskLink could not look for the item\'s task', {
        error,
        itemId: props.itemId,
      }),
    },
  )
  const task = data?.task
  if (!task) {
    return null
  }

  const title = task.meetingTitle.trim()
  return <section className='relaxed'>
    <h2 className='compact text-lg font-semibold'>{t('taskLink.heading')}</h2>
    <Link
      href={getMeetingMarkUrl(task.meetingId, task.taskId)}
      className={buttonVariants({ variant: 'outline' })}
    >
      <LuTextSearch className='size-4' aria-hidden />
      <span>{title
        ? t('taskLink.open', { title })
        : t('taskLink.openUntitled')}</span>
    </Link>
  </section>
}
