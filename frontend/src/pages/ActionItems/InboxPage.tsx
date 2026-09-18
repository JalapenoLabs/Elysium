// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectInboxActionItems } from '../../store/actionItemsSlice'

// User interface
import { buttonVariants, Link, Spinner } from '@heroui/react'
import { LuInbox } from 'react-icons/lu'
import { InboxTriageCard } from './InboxTriageCard'

// Misc
import { useActionItemsLoader } from '../../hooks/useServerData'
import { useNow } from '../../hooks/useNow'
import { getActionItemViewUrl, UrlTree } from '../../urls'

// How many of the items after the current one are listed below it.
const UPCOMING_LIMIT = 5

// `/action-items/inbox`: triage, one item at a time, oldest first. Accepting moves an item
// to Next and dismissing drops it; either way the next one takes its place.
export function InboxPage() {
  const { t } = useTranslation('actionItems')
  const status = useActionItemsLoader()
  const now = useNow()
  const inbox = useAppSelector(selectInboxActionItems)

  if (status === 'loading') {
    return <div className='grid place-items-center py-16'>
      <Spinner />
    </div>
  }
  if (status === 'failed') {
    return <p className='py-10 text-center text-sm text-danger'>{t('next.loadError')}</p>
  }

  const [ current, ...rest ] = inbox
  if (!current) {
    return <div className='rounded-xl border border-separator px-6 py-14 text-center'>
      <LuInbox className='mx-auto mb-3 size-10 opacity-50' aria-hidden />
      <h2 className='compact text-xl font-semibold'>{t('inbox.emptyTitle')}</h2>
      <p className='compact text-sm opacity-70'>{t('inbox.emptyBody')}</p>
      <Link href={UrlTree.actionItems} className={buttonVariants({ size: 'sm' })}>
        <span>{t('inbox.backToNext')}</span>
      </Link>
    </div>
  }

  return <div>
    <p className='compact text-sm opacity-70'>{t('inbox.remaining', { count: inbox.length })}</p>
    <InboxTriageCard key={current.id} item={current} now={now} />
    {rest.length > 0 && <section>
      <h2 className='compact text-sm font-semibold opacity-70'>{t('inbox.upNext')}</h2>
      <ol className='flex flex-col gap-1 text-sm'>{
        rest.slice(0, UPCOMING_LIMIT).map((item) => <li key={item.id}>
          <Link href={getActionItemViewUrl(item.id)} className='text-link no-underline hover:underline'>{
            item.title
          }</Link>
        </li>)
      }</ol>
      {rest.length > UPCOMING_LIMIT && <p className='mt-1 text-xs opacity-60'>{
        t('inbox.andMore', { count: rest.length - UPCOMING_LIMIT })
      }</p>}
    </section>}
  </div>
}
