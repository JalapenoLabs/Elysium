// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { buttonVariants, Card, Link } from '@heroui/react'
import { LuInbox } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../urls'

type Props = {
  count: number
}

// Leads Next while the inbox holds anything: new arrivals are decided on before the day's
// work, so nothing waits unseen.
export function InboxLeadCard(props: Props) {
  const { t } = useTranslation('actionItems')

  return <Card className='relaxed'>
    <Card.Content className='flex-row flex-wrap items-center justify-between gap-4'>
      <div className='flex items-center gap-3 text-left'>
        <LuInbox className='size-5 shrink-0 text-accent' aria-hidden />
        <p className='font-semibold'>{t('inboxLead.title', { count: props.count })}</p>
      </div>
      <Link href={UrlTree.actionItemsInbox} className={buttonVariants({ size: 'sm' })}>
        <span>{t('inboxLead.action')}</span>
      </Link>
    </Card.Content>
  </Card>
}
