// Copyright © 2026 Jalapeno Labs

import type { EventStreamConnection } from '../store/realtimeSlice'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../store/hooks'
import { selectEventStreamConnection } from '../store/realtimeSlice'

// User interface
import { Tooltip } from '@heroui/react'

const dotClassNames = {
  connecting: 'bg-muted animate-pulse',
  open: 'bg-success',
  reconnecting: 'bg-warning animate-pulse',
} as const satisfies Record<EventStreamConnection, string>

const labelKeys = {
  connecting: 'topbar.liveUpdates.connecting',
  open: 'topbar.liveUpdates.open',
  reconnecting: 'topbar.liveUpdates.reconnecting',
} as const satisfies Record<EventStreamConnection, string>

// A dot showing whether the event stream is connected. While it is not, what is on
// screen can be stale, which is worth a glance-level signal.
export function LiveUpdatesIndicator() {
  const { t } = useTranslation('navigation')
  const connection = useAppSelector(selectEventStreamConnection)

  return <Tooltip delay={200}>
    <Tooltip.Trigger aria-label={t(labelKeys[connection])} className='grid size-8 place-items-center'>
      <span className={`block size-2 rounded-full ${dotClassNames[connection]}`} />
    </Tooltip.Trigger>
    <Tooltip.Content>
      <span>{t(labelKeys[connection])}</span>
    </Tooltip.Content>
  </Tooltip>
}
