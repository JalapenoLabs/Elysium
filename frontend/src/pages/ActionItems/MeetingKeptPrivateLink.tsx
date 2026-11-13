// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { buttonVariants, Link } from '@heroui/react'
import { LuLock } from 'react-icons/lu'

// Misc
import { listMeetingsKeptPrivateBy } from '../../api/routes/meetingRoutes'
import { getMeetingUrl } from '../../urls'
import { correctionDecisionOf } from '../Knowledge/Meetings/meetingVisibility'

type Props = {
  itemId: string
}

// The meeting an item told its owner its correction kept private, with what the correction said,
// linked to its page, where it is opened to everyone or shared. The item names nothing of it, since
// items are the whole workspace's; this shows it to those who may read it alone, and nothing to
// anyone else, or for an item about no such meeting.
export function MeetingKeptPrivateLink(props: Props) {
  const { t } = useTranslation('meetings')
  const { data } = useSWR(
    [ 'v1/meetings/kept-private-by', props.itemId ],
    () => listMeetingsKeptPrivateBy(props.itemId),
    {
      onError: (error: unknown) => console.debug('MeetingKeptPrivateLink could not look for the item\'s meeting', {
        error,
        itemId: props.itemId,
      }),
    },
  )
  const meeting = data?.meetings[0]
  if (!meeting) {
    return null
  }
  const decision = correctionDecisionOf(meeting.visibilityChange)

  return <section className='relaxed'>
    <h2 className='compact text-lg font-semibold'>{t('visibility.keptPrivateItem.heading')}</h2>
    <p className='compact font-medium'>{meeting.title || t('list.untitled')}</p>
    {decision && <p className='compact text-sm opacity-80'>{t(decision.key, { why: decision.why ?? '' })}</p>}
    <Link href={getMeetingUrl(meeting.id)} className={buttonVariants({ variant: 'outline' })}>
      <LuLock className='size-4' aria-hidden />
      <span>{t('visibility.keptPrivateItem.open')}</span>
    </Link>
  </section>
}
