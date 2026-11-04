// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { buttonVariants, Link } from '@heroui/react'
import { LuFileCheck } from 'react-icons/lu'

// Misc
import { listMeetingsReviewedBy } from '../../api/routes/meetingRoutes'
import { getMeetingUrl } from '../../urls'

type Props = {
  itemId: string
}

// The meeting an item asks its person to approve the corrected transcript of, linked to its
// page, where the transcript and Approve are. An item that asks for no review, or one asking
// someone else, shows nothing: a meeting is its person's alone.
export function MeetingReviewLink(props: Props) {
  const { t } = useTranslation('meetings')
  const { data } = useSWR(
    [ 'v1/meetings/reviewed-by', props.itemId ],
    () => listMeetingsReviewedBy(props.itemId),
    {
      onError: (error: unknown) => console.debug('MeetingReviewLink could not look for the item\'s meeting', {
        error,
        itemId: props.itemId,
      }),
    },
  )
  const meeting = data?.meetings[0]
  if (!meeting) {
    return null
  }

  return <section className='relaxed'>
    <h2 className='compact text-lg font-semibold'>{t('review.heading')}</h2>
    <Link href={getMeetingUrl(meeting.id)} className={buttonVariants({ variant: 'outline' })}>
      <LuFileCheck className='size-4' aria-hidden />
      <span>{t('review.open')}</span>
    </Link>
  </section>
}
