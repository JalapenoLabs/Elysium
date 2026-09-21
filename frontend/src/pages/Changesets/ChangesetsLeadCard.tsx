// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { buttonVariants, Card, Link } from '@heroui/react'
import { LuGitPullRequestArrow } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../urls'

type Props = {
  count: number
}

// Leads Next while changesets wait for review, like the inbox's card: proposed changes are
// decided on before the day's work, so nothing an agent or Elysia proposed waits unseen.
export function ChangesetsLeadCard(props: Props) {
  const { t } = useTranslation('changesets')

  return <Card className='relaxed'>
    <Card.Content className='flex-row flex-wrap items-center justify-between gap-4'>
      <div className='flex items-center gap-3 text-left'>
        <LuGitPullRequestArrow className='size-5 shrink-0 text-accent' aria-hidden />
        <div>
          <p className='font-semibold'>{t('lead.title', { count: props.count })}</p>
          <p className='text-sm opacity-70'>{t('lead.body')}</p>
        </div>
      </div>
      <Link href={UrlTree.changesets} className={buttonVariants({ size: 'sm' })}>
        <span>{t('lead.action')}</span>
      </Link>
    </Card.Content>
  </Card>
}
