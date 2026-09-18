// Copyright © 2026 Jalapeno Labs

import type { Initiative } from '../../api/routes/initiativeRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Spinner } from '@heroui/react'
import { BurnupChart } from './BurnupChart'
import { InitiativeProgressSummary } from './InitiativeProgressSummary'

// Misc
import { useInitiativeProgress } from '../../hooks/useServerData'

type Props = {
  initiative: Initiative
}

// Progress on an initiative's page: resolved and total now, from the initiative in Redux,
// and the burnup of both over time beneath them.
export function InitiativeProgressSection(props: Props) {
  const { t } = useTranslation('initiatives')
  const { progress, status } = useInitiativeProgress(props.initiative.id)

  return <div>
    <div className='compact'>
      <InitiativeProgressSummary progress={props.initiative.progress} size='lg' />
    </div>
    {status === 'loading' && <div className='grid place-items-center py-10'>
      <Spinner size='sm' />
    </div>}
    {status === 'failed' && <p className='text-sm text-danger'>{t('burnup.loadError')}</p>}
    {progress && progress.burnup.length > 0 && <BurnupChart points={progress.burnup} />}
  </div>
}
