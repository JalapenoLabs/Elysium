// Copyright © 2026 Jalapeno Labs

import type { WorkerPool } from '../../api/routes/workQueueRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Alert, Tooltip } from '@heroui/react'
import { ConfigurationHoldAlert } from '../../components/ConfigurationHoldAlert'

// Misc
import { formatDateTime, formatRelativeTime } from '../../format/dates'
import { useNow } from '../../hooks/useNow'

type Props = {
  pool: WorkerPool
}

// How busy the pool is, why no worker starts while the configuration repository holds them back,
// and, while every Claude account is at its usage limit, when its workers start again: standing
// state, there whenever the page opens.
export function WorkerPoolSummary(props: Props) {
  const { t } = useTranslation('workQueue')
  const now = useNow()

  return <div className='relaxed'>
    <p className='compact text-sm opacity-80'>{
      props.pool.size === null
        ? t('pool.busyUnsized', { count: props.pool.inUse })
        : t('pool.busy', { count: props.pool.size, inUse: props.pool.inUse })
    }</p>
    {props.pool.configurationHold && <ConfigurationHoldAlert hold={props.pool.configurationHold} />}
    {props.pool.usageHeldUntil && <Alert status='warning' className='max-w-3xl'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>
          <Tooltip delay={300}>
            <Tooltip.Trigger>
              <span>{
                t('pool.usageHeld', { time: formatRelativeTime(props.pool.usageHeldUntil, now) })
              }</span>
            </Tooltip.Trigger>
            <Tooltip.Content>{formatDateTime(props.pool.usageHeldUntil)}</Tooltip.Content>
          </Tooltip>
        </Alert.Title>
      </Alert.Content>
    </Alert>}
  </div>
}
