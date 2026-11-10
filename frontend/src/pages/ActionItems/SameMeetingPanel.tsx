// Copyright © 2026 Jalapeno Labs

import type { MeetingDuplicate } from '../../api/routes/meetingRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

// User interface
import { Button, buttonVariants, Link } from '@heroui/react'
import { LuCombine, LuSplit } from 'react-icons/lu'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../api/errors'
import {
  getMeetingDuplicateAskedBy,
  keepMeetingDuplicateApart,
  mergeMeetingDuplicate,
} from '../../api/routes/meetingRoutes'
import { formatDateTime } from '../../format/dates'
import { toast } from '../../toast/toast'
import { getMeetingUrl } from '../../urls'

type Props = {
  itemId: string
}

// The two meetings an item asks its person whether they are one, each linked to its page, with
// Merge and Not the same until they answer and what they answered after. An item that asks about
// none of theirs shows nothing: a meeting is its person's alone.
export function SameMeetingPanel(props: Props) {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const { data, mutate } = useSWR(
    [ 'v1/meetings/duplicates', props.itemId ],
    () => getMeetingDuplicateAskedBy(props.itemId),
    {
      onError: (error: unknown) => console.debug('SameMeetingPanel could not look for the item\'s meetings', {
        error,
        itemId: props.itemId,
      }),
    },
  )
  const [ acting, setActing ] = useState<'merge' | 'apart' | null>(null)
  const duplicate = data?.duplicate
  if (!duplicate) {
    return null
  }

  async function decide(answer: 'merge' | 'apart', pair: MeetingDuplicate) {
    setActing(answer)
    try {
      const response = answer === 'merge'
        ? await mergeMeetingDuplicate(pair.id)
        : await keepMeetingDuplicateApart(pair.id)
      await mutate({ duplicate: response.duplicate }, { revalidate: false })
      toast.success(answer === 'merge'
        ? t('sameMeeting.mergedToast')
        : t('sameMeeting.keptApartToast'))
    }
    catch (error) {
      console.debug('SameMeetingPanel could not record an answer', { error, answer, duplicateId: pair.id })
      if (getApiErrorCode(error) === 'duplicate_decided') {
        toast.warning(t('sameMeeting.decidedMeanwhile'))
        await mutate()
        return
      }
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setActing(null)
    }
  }

  return <section className='relaxed'>
    <h2 className='compact text-lg font-semibold'>{t('sameMeeting.heading')}</h2>
    <div className='compact flex flex-wrap gap-2'>
      <Link href={getMeetingUrl(duplicate.meetingId)} className={buttonVariants({ variant: 'outline' })}>
        <span>{duplicate.meetingTitle || t('sameMeeting.untitled')}</span>
      </Link>
      <Link href={getMeetingUrl(duplicate.sameAsId)} className={buttonVariants({ variant: 'outline' })}>
        <span>{duplicate.sameAsTitle || t('sameMeeting.untitled')}</span>
      </Link>
    </div>
    {duplicate.decision
      ? <p className='opacity-80'>{
          t(duplicate.decision === 'merged'
            ? 'sameMeeting.merged'
            : 'sameMeeting.keptApart', {
            date: duplicate.decidedAt
              ? formatDateTime(duplicate.decidedAt)
              : '',
          })
        }</p>
      : <div className='flex flex-wrap gap-2'>
        <Button
          isPending={acting === 'merge'}
          isDisabled={acting !== null}
          onPress={() => void decide('merge', duplicate)}
        >
          <LuCombine className='size-4' aria-hidden />
          <span>{t('sameMeeting.merge')}</span>
        </Button>
        <Button
          variant='outline'
          isPending={acting === 'apart'}
          isDisabled={acting !== null}
          onPress={() => void decide('apart', duplicate)}
        >
          <LuSplit className='size-4' aria-hidden />
          <span>{t('sameMeeting.notTheSame')}</span>
        </Button>
      </div>}
  </section>
}
