// Copyright © 2026 Jalapeno Labs

import type { BrowserSession } from '../../../api/routes/browserSessionRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Card, Chip, Link, buttonVariants } from '@heroui/react'

// Misc
import { getBrowserSessionUrl } from '../../../urls'
import { stateChipColors, stateLabelKeys } from './browserPresentation'

type Props = {
  browserSession: BrowserSession
  onDelete: (browserSession: BrowserSession) => void
}

export function BrowserSessionCard(props: Props) {
  const { t } = useTranslation([ 'browsers', 'common' ])
  const { status } = props.browserSession
  const host = URL.canParse(props.browserSession.startUrl)
    ? new URL(props.browserSession.startUrl).host
    : props.browserSession.startUrl

  return <Card className='compact p-4'>
    <div className='level items-start gap-4'>
      <div className='min-w-0'>
        <div className='flex items-center gap-2'>
          <h2 className='truncate text-base font-semibold'>{
            props.browserSession.name
          }</h2>
          <Chip size='sm' variant='soft' color={stateChipColors[status.state]}>{
            t(stateLabelKeys[status.state])
          }</Chip>
        </div>
        <p className='truncate text-sm opacity-70'>{host}</p>
        {status.error && <p className='mt-1 text-sm text-danger'>{status.error}</p>}
      </div>
      <div className='flex shrink-0 gap-2'>
        <Link
          href={getBrowserSessionUrl(props.browserSession.id)}
          className={buttonVariants({ size: 'sm', variant: 'outline' })}
        >
          {t('card.open')}
        </Link>
        <Button size='sm' variant='danger-soft' onPress={() => props.onDelete(props.browserSession)}>
          <span>{t('common:actions.delete')}</span>
        </Button>
      </div>
    </div>
  </Card>
}
