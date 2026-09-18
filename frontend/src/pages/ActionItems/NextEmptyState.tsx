// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectLiveActionItems } from '../../store/actionItemsSlice'

// User interface
import { buttonVariants, Link } from '@heroui/react'
import { LuCircleCheck } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../urls'
import { isSnoozed } from './actionItemPresentation'

type Props = {
  now: number
}

// Next with nothing in it. Says where the rest of the open work is: waiting on someone, or
// snoozed until later.
export function NextEmptyState(props: Props) {
  const { t } = useTranslation('actionItems')
  const items = useAppSelector(selectLiveActionItems)

  let waitingCount = 0
  let snoozedCount = 0
  for (const item of items) {
    if (item.state !== 'open') {
      continue
    }
    if (item.waitingOn) {
      waitingCount++
    }
    if (isSnoozed(item, props.now)) {
      snoozedCount++
    }
  }

  return <div className='rounded-xl border border-separator px-6 py-14 text-center'>
    <LuCircleCheck className='mx-auto mb-3 size-10 text-success' aria-hidden />
    <h2 className='compact text-xl font-semibold'>{t('next.emptyTitle')}</h2>
    <p className='compact text-sm opacity-70'>{t('next.emptyBody')}</p>
    {(waitingCount > 0 || snoozedCount > 0) && <p className='compact text-sm opacity-70'>{
      t('next.emptyElsewhere', {
        waiting: t('next.waitingCount', { count: waitingCount }),
        snoozed: t('next.snoozedCount', { count: snoozedCount }),
      })
    }</p>}
    <div className='mt-4 flex justify-center gap-2'>
      <Link href={UrlTree.actionItemsNew} className={buttonVariants({ size: 'sm' })}>
        <span>{t('newItem')}</span>
      </Link>
      <Link href={UrlTree.actionItemsAll} className={buttonVariants({ size: 'sm', variant: 'outline' })}>
        <span>{t('next.seeAll')}</span>
      </Link>
    </div>
  </div>
}
