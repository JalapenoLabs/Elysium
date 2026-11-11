// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { buttonVariants, Link } from '@heroui/react'
import { LuMessageSquare } from 'react-icons/lu'

// Misc
import { getThreadOfItem } from '../../api/routes/meetingRoutes'
import { getMeetingThreadUrl } from '../../urls'

type Props = {
  itemId: string
}

// The comment thread on a meeting an item was raised by (a mention, a reply, a resolve, or a
// reopen), linked to the meeting's page opened at it. An item no thread raised, or one about a
// meeting of someone else's, shows nothing: a meeting is its person's alone.
export function MeetingThreadLink(props: Props) {
  const { t } = useTranslation('meetings')
  const { data } = useSWR(
    [ 'v1/meetings/thread-items', props.itemId ],
    () => getThreadOfItem(props.itemId),
    {
      onError: (error: unknown) => console.debug('MeetingThreadLink could not look for the item\'s thread', {
        error,
        itemId: props.itemId,
      }),
    },
  )
  const thread = data?.thread
  if (!thread) {
    return null
  }

  return <section className='relaxed'>
    <h2 className='compact text-lg font-semibold'>{t('threadLink.heading')}</h2>
    <Link
      href={getMeetingThreadUrl(thread.meetingId, thread.threadId)}
      className={buttonVariants({ variant: 'outline' })}
    >
      <LuMessageSquare className='size-4' aria-hidden />
      <span>{t('threadLink.open')}</span>
    </Link>
  </section>
}
